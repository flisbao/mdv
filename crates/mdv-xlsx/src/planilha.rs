//! Leitura de uma aba em fluxo: `Vec<Celula>` por linha não vazia, colunas recortadas e tetos.
//! Nunca se indexa pelo número de linha ou de coluna declarado no arquivo: linhas são compactadas,
//! colunas além de `max_colunas` são descartadas e cada texto gasta do orçamento de bytes.
//! Fórmula: só o `<v>` (valor em cache) é lido; `<f>` é pulado sem ser interpretado.

use crate::celula::{acumular_limitado, converter, texto_seguro, Celula};
use crate::estilos::Estilos;
use crate::limites::Limites;
use crate::referencia::coluna_de;
use crate::xml_leitor::{Evento, Leitor};
use crate::Budget;

/// Qual teto cortou a leitura da aba (o primeiro que disparou).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Corte {
    /// Orçamento de bytes de texto de células (compartilhado entre as abas).
    Bytes,
    /// `max_celulas`: células guardadas ou tamanho da grade.
    Celulas,
}

/// Uma aba lida: linhas já com a mesma largura, mais o que foi cortado.
#[derive(Debug, Default)]
pub(crate) struct Aba {
    pub linhas: Vec<Vec<Celula>>,
    /// Linhas não vazias lidas, inclusive as que passaram de `max_linhas` (não guardadas).
    pub linhas_total: usize,
    pub colunas_omitidas: bool,
    pub corte: Option<Corte>,
}

/// Lê uma aba (`sheetData` de um `worksheet`). `orcamento` são os bytes de célula ainda
/// disponíveis, compartilhados entre as abas; esgotado, a leitura para com `corte = Some(Corte::Bytes)`.
pub(crate) fn ler(
    xml: &[u8],
    textos: &[String],
    estilos: &Estilos,
    data_1904: bool,
    limites: &Limites,
    orcamento: &mut usize,
    budget: &dyn Budget,
) -> Result<Aba, String> {
    let mut leitor = Leitor::novo(xml, budget);
    let mut montagem = Montagem::novo(limites.max_linhas, limites.max_celulas);
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
                if coluna >= limites.max_colunas {
                    montagem.aba.colunas_omitidas = true;
                    continue;
                }
                let estilo = tag.attr("s").and_then(|s| s.parse().ok()).unwrap_or(0);
                // Número cortado não é lido como número: o prefixo daria outro valor.
                let celula = if cortado && matches!(tag.attr("t"), None | Some("n")) {
                    Celula { texto: bruto, numerica: false }
                } else {
                    converter(tag.attr("t"), &bruto, estilos.formato(estilo), textos, data_1904)
                };
                let texto = texto_seguro(&celula.texto, limites.max_celula_chars);
                let custo = texto.len().max(1);
                if custo > *orcamento {
                    montagem.aba.corte.get_or_insert(Corte::Bytes);
                    break;
                }
                *orcamento -= custo;
                // A faixa de colunas inclui as células vazias (fórmula sem valor ocupa a coluna).
                linha.faixa = alarga(linha.faixa, coluna);
                guardar(&mut linha.celulas, coluna, Celula { texto, numerica: celula.numerica });
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

/// Conteúdo de uma `<c>` já aberta, até o `Fecha` dela: `f` é pulado; `v`, o texto de `is` e o
/// `t` dentro dele formam o valor bruto, limitado a `max_chars`. Célula sem `v` devolve vazio.
/// O segundo item diz se o valor foi cortado.
fn ler_valor(leitor: &mut Leitor, max_chars: usize) -> Result<(String, bool), String> {
    let mut valor = String::new();
    let mut cortado = false;
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
                    cortado |= acumular_limitado(&mut valor, &trecho, max_chars);
                }
            }
            Evento::Fecha(nome) => match nome.as_str() {
                "c" => return Ok((valor, cortado)),
                "v" => em_v = false,
                "t" => em_t = false,
                _ => {}
            },
        }
    }
}

/// Guarda uma célula na linha: uma por coluna, a última vence no lugar; uma vazia apaga a anterior.
/// Assim a linha guarda no máximo uma entrada por coluna, mesmo com a coluna repetida no XML.
pub(crate) fn guardar(celulas: &mut Vec<(usize, Celula)>, coluna: usize, celula: Celula) {
    let posicao = celulas.iter().position(|(c, _)| *c == coluna);
    match (posicao, celula.texto.is_empty()) {
        (Some(i), true) => {
            celulas.swap_remove(i);
        }
        (Some(i), false) => celulas[i].1 = celula,
        (None, true) => {}
        (None, false) => celulas.push((coluna, celula)),
    }
}

/// Linha em construção: células não vazias (coluna, célula) e a faixa de colunas tocadas.
#[derive(Default)]
struct Linha {
    celulas: Vec<(usize, Celula)>,
    faixa: Option<(usize, usize)>,
}

fn alarga(faixa: Option<(usize, usize)>, coluna: usize) -> Option<(usize, usize)> {
    Some(match faixa {
        Some((menor, maior)) => (menor.min(coluna), maior.max(coluna)),
        None => (coluna, coluna),
    })
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
    menor: usize,
    maior: usize,
    max_linhas: usize,
    max_celulas: usize,
}

impl Montagem {
    fn novo(max_linhas: usize, max_celulas: usize) -> Self {
        Montagem {
            aba: Aba::default(),
            retidas: Vec::new(),
            celulas_retidas: 0,
            cheia: false,
            menor: usize::MAX,
            maior: 0,
            max_linhas,
            max_celulas,
        }
    }

    /// Fecha a linha: sem célula não vazia, descarta; senão conta e guarda até os tetos.
    fn fechar(&mut self, linha: &mut Linha) {
        let Linha { celulas, faixa } = std::mem::take(linha);
        if celulas.is_empty() {
            return;
        }
        self.aba.linhas_total += 1;
        if self.retidas.len() >= self.max_linhas || self.cheia {
            return;
        }
        if self.celulas_retidas + celulas.len() > self.max_celulas {
            self.cheia = true;
            self.aba.corte.get_or_insert(Corte::Celulas);
            return;
        }
        self.celulas_retidas += celulas.len();
        if let Some((menor, maior)) = faixa {
            self.menor = self.menor.min(menor);
            self.maior = self.maior.max(maior);
        }
        self.retidas.push(celulas);
    }

    /// Monta a grade. Passado `max_celulas / largura` linhas (pelo menos uma), corta as de baixo;
    /// `linhas_total` continua contando todas.
    fn finalizar(self) -> Aba {
        let Montagem { mut aba, mut retidas, menor, maior, max_celulas, .. } = self;
        if retidas.is_empty() {
            return aba;
        }
        let largura = maior - menor + 1;
        let couberam = (max_celulas / largura).max(1);
        if retidas.len() > couberam {
            retidas.truncate(couberam);
            aba.corte.get_or_insert(Corte::Celulas);
        }
        aba.linhas = retidas
            .into_iter()
            .map(|celulas| {
                let mut grade: Vec<Celula> = vec![Celula::default(); largura];
                for (coluna, celula) in celulas {
                    grade[coluna - menor] = celula;
                }
                grade
            })
            .collect();
        aba
    }
}
