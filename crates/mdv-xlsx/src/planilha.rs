//! Leitura de uma aba em fluxo: `Vec<Celula>` por linha não vazia, colunas recortadas e tetos.
//! Nunca se indexa pelo número de linha ou de coluna declarado no arquivo: linhas são compactadas,
//! colunas além de `max_colunas` são descartadas e cada texto gasta do orçamento de bytes.
//! Este é o único ponto que sanitiza e corta o texto vindo do arquivo (`texto_seguro`): os
//! valores crus de `ler_valor` e os textos compartilhados de `textos::ler` chegam aqui só limitados.
//! Fórmula: só o `<v>` (valor em cache) é lido; `<f>` é pulado sem ser interpretado.

use crate::celula::interpretar;
use crate::estilos::Estilos;
use crate::limites::{Limites, Orcamento};
use crate::modelo::{Aba, Celula, Corte};
use crate::referencia::coluna_de;
use crate::texto_limitado::{texto_seguro, TextoLimitado};
use crate::xml_leitor::{Evento, Leitor, Tag};
use crate::Budget;

/// O que é comum a todas as abas da pasta de trabalho: textos compartilhados, estilos, sistema
/// de datas e tetos. Montado uma vez e emprestado a cada `ler`.
pub(crate) struct ContextoPasta<'a> {
    pub textos: &'a [String],
    pub estilos: &'a Estilos,
    pub data_1904: bool,
    pub limites: &'a Limites,
}

/// Lê uma aba (`sheetData` de um `worksheet`). `orcamento` são os bytes de célula ainda
/// disponíveis, compartilhados entre as abas; esgotado, a leitura para com `corte = Some(Corte::Bytes)`.
pub(crate) fn ler(xml: &[u8], ctx: &ContextoPasta, orcamento: &mut Orcamento, budget: &dyn Budget) -> Result<Aba, String> {
    let limites = ctx.limites;
    let mut leitor = Leitor::novo(xml, budget);
    let mut montagem = Montagem::nova(limites.max_linhas, limites.max_celulas);
    let mut linha = Linha::default();
    let mut em_linha = false;
    let mut proxima = 0usize;
    while let Some(evento) = leitor.proximo()? {
        match evento {
            Evento::Abre(tag) if tag.nome == "row" => {
                em_linha = true;
                proxima = 0;
            }
            Evento::Abre(tag) if tag.nome == "c" && em_linha => {
                // Sem `r` a coluna é a seguinte à anterior, como o Excel faz.
                let coluna = tag.attr("r").and_then(coluna_de).unwrap_or(proxima);
                proxima = coluna.saturating_add(1);
                let (bruto, cortado) = ler_valor(&mut leitor, limites.max_celula_chars)?;
                // Coluna além do teto: descartada sem cobrar o orçamento.
                if coluna >= limites.max_colunas {
                    montagem.aba.colunas_omitidas = Some(limites.max_colunas);
                    continue;
                }
                let celula = celula_de(&tag, bruto, cortado, ctx);
                if !orcamento.gastar(celula.texto.len().max(1)) {
                    montagem.aba.corte.get_or_insert(Corte::Bytes);
                    break;
                }
                linha.registrar(coluna, celula);
            }
            Evento::Fecha(nome) if nome == "row" => {
                em_linha = false;
                montagem.fechar(&mut linha);
            }
            _ => {}
        }
    }
    // Linha interrompida pelo orçamento ainda entra com o que já foi lido.
    montagem.fechar(&mut linha);
    Ok(montagem.finalizar())
}

/// Regra de uma célula: tipo (`t`) e estilo (`s`) da tag + valor bruto → texto final, já
/// sanitizado e cortado em `max_celula_chars`.
fn celula_de(tag: &Tag, bruto: String, cortado: bool, ctx: &ContextoPasta) -> Celula {
    let estilo = tag.attr("s").and_then(|s| s.parse().ok()).unwrap_or(0);
    // Número cortado não é lido como número: o prefixo daria outro valor.
    let celula = if cortado && matches!(tag.attr("t"), None | Some("n")) {
        Celula { texto: bruto, numerica: false }
    } else {
        interpretar(tag.attr("t"), &bruto, estilo, ctx)
    };
    Celula { texto: texto_seguro(&celula.texto, ctx.limites.max_celula_chars), numerica: celula.numerica }
}

/// Conteúdo de uma `<c>` já aberta, até o `Fecha` dela: `f` é pulado; `v`, o texto de `is` e o
/// `t` dentro dele formam o valor bruto, limitado a `max_chars`. Célula sem `v` devolve vazio.
/// O segundo item diz se o valor foi cortado.
fn ler_valor(leitor: &mut Leitor, max_chars: usize) -> Result<(String, bool), String> {
    let mut valor = TextoLimitado::novo(max_chars);
    let (mut em_v, mut em_t) = (false, false);
    loop {
        // Defensivo: o `Leitor` já recusa o fim truncado (erro "XML inválido") com `<c>` aberta,
        // então este `None` não deveria ocorrer; a mensagem existe só para não haver `unwrap`.
        let evento = leitor.proximo()?.ok_or_else(|| "XML inválido: célula sem fechamento".to_string())?;
        match evento {
            Evento::Abre(tag) => match tag.nome.as_str() {
                "v" => em_v = true,
                "t" => em_t = true,
                "is" => {}
                // Fórmula (`f`), fonético (`rPh`) e o que mais houver: pulados sem ler.
                _ => leitor.pular()?,
            },
            Evento::Texto(trecho) => {
                if em_v || em_t {
                    valor.acrescentar(&trecho);
                }
            }
            Evento::Fecha(nome) => match nome.as_str() {
                "c" => {
                    let cortado = valor.cortado();
                    return Ok((valor.into_bruto(), cortado));
                }
                "v" => em_v = false,
                "t" => em_t = false,
                _ => {}
            },
        }
    }
}

/// Faixa de colunas tocadas por uma linha (ou por todas as retidas): `menor <= maior` sempre.
#[derive(Clone, Copy)]
struct Faixa {
    menor: usize,
    maior: usize,
}

impl Faixa {
    fn com(coluna: usize) -> Faixa {
        Faixa { menor: coluna, maior: coluna }
    }

    fn alargar(self, coluna: usize) -> Faixa {
        self.unir(Faixa::com(coluna))
    }

    fn unir(self, outra: Faixa) -> Faixa {
        Faixa { menor: self.menor.min(outra.menor), maior: self.maior.max(outra.maior) }
    }

    fn largura(self) -> usize {
        self.maior - self.menor + 1
    }
}

/// Linha em construção: células não vazias (coluna, célula) e a faixa de colunas tocadas.
#[derive(Default)]
pub(crate) struct Linha {
    pub(crate) celulas: Vec<(usize, Celula)>,
    faixa: Option<Faixa>,
}

impl Linha {
    /// Guarda uma célula: uma por coluna, a última vence no lugar; uma vazia apaga a anterior.
    /// Assim a linha guarda no máximo uma entrada por coluna, mesmo com a coluna repetida no XML.
    /// A faixa de colunas inclui as células vazias (fórmula sem valor ocupa a coluna).
    pub(crate) fn registrar(&mut self, coluna: usize, celula: Celula) {
        self.faixa = Some(match self.faixa {
            Some(faixa) => faixa.alargar(coluna),
            None => Faixa::com(coluna),
        });
        let posicao = self.celulas.iter().position(|(c, _)| *c == coluna);
        match (posicao, celula.texto.is_empty()) {
            (Some(i), true) => {
                self.celulas.swap_remove(i);
            }
            (Some(i), false) => self.celulas[i].1 = celula,
            (None, true) => {}
            (None, false) => self.celulas.push((coluna, celula)),
        }
    }
}

/// Guarda as linhas (esparsas) e, no fim, monta a grade com a largura da faixa usada.
/// Guardar para de crescer ao passar de `max_linhas` linhas ou de `max_celulas` células guardadas
/// (a conta é das células guardadas, não da grade), então a memória não depende da largura.
struct Montagem {
    aba: Aba,
    retidas: Vec<Vec<(usize, Celula)>>,
    celulas_retidas: usize,
    /// Já cortou por `max_celulas`: as linhas seguintes não entram, para o que sobra ser um prefixo.
    cheia: bool,
    /// Faixa das linhas retidas; `None` enquanto não houver nenhuma.
    faixa: Option<Faixa>,
    max_linhas: usize,
    max_celulas: usize,
}

impl Montagem {
    fn nova(max_linhas: usize, max_celulas: usize) -> Self {
        Montagem { aba: Aba::default(), retidas: Vec::new(), celulas_retidas: 0, cheia: false, faixa: None, max_linhas, max_celulas }
    }

    /// Fecha a linha: sem célula não vazia, descarta; senão conta e guarda até os tetos.
    fn fechar(&mut self, linha: &mut Linha) {
        let Linha { celulas, faixa } = std::mem::take(linha);
        // Sem faixa nenhuma célula foi registrada; com faixa mas sem texto (só fórmulas sem valor) também descarta.
        let Some(faixa) = faixa else { return };
        if celulas.is_empty() {
            return;
        }
        self.aba.linhas_total += 1;
        if self.retidas.len() >= self.max_linhas || self.cheia {
            return;
        }
        if self.celulas_retidas + celulas.len() > self.max_celulas {
            self.cheia = true;
            self.aba.corte.get_or_insert(Corte::Celulas { teto: self.max_celulas });
            return;
        }
        self.celulas_retidas += celulas.len();
        self.faixa = Some(match self.faixa {
            Some(atual) => atual.unir(faixa),
            None => faixa,
        });
        self.retidas.push(celulas);
    }

    /// Monta a grade. Passado `max_celulas / largura` linhas (pelo menos uma), corta as de baixo;
    /// `linhas_total` continua contando todas.
    fn finalizar(self) -> Aba {
        let Montagem { mut aba, mut retidas, faixa, max_celulas, .. } = self;
        let Some(faixa) = faixa else { return aba };
        let largura = faixa.largura();
        let couberam = (max_celulas / largura).max(1);
        if retidas.len() > couberam {
            retidas.truncate(couberam);
            aba.corte.get_or_insert(Corte::Celulas { teto: max_celulas });
        }
        aba.linhas = retidas
            .into_iter()
            .map(|celulas| {
                let mut grade: Vec<Celula> = vec![Celula::default(); largura];
                for (coluna, celula) in celulas {
                    grade[coluna - faixa.menor] = celula;
                }
                grade
            })
            .collect();
        aba
    }
}
