//! Leitura de uma aba em fluxo: `Vec<Celula>` por linha não vazia, colunas recortadas e tetos.
//! Nunca se indexa pelo número de linha ou de coluna declarado no arquivo: linhas são compactadas,
//! colunas além de `max_colunas` são descartadas e cada texto gasta do orçamento de bytes.
//! Fórmula: só o `<v>` (valor em cache) é lido; `<f>` é pulado sem ser interpretado.

use crate::celula::{converter, texto_seguro, Celula};
use crate::estilos::Estilos;
use crate::limites::Limites;
use crate::referencia::coluna_de;
use crate::xml_leitor::{Evento, Leitor};
use crate::Budget;

/// Uma aba lida: linhas já com a mesma largura, mais o que foi cortado.
#[derive(Debug, Default)]
pub(crate) struct Aba {
    pub linhas: Vec<Vec<Celula>>,
    /// Linhas não vazias lidas, inclusive as que passaram de `max_linhas` (não guardadas).
    pub linhas_total: usize,
    pub colunas_omitidas: bool,
    pub cortada: bool,
}

/// Lê uma aba (`sheetData` de um `worksheet`). `orcamento` são os bytes de célula ainda
/// disponíveis, compartilhados entre as abas; esgotado, a leitura para com `cortada = true`.
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
    let mut montagem = Montagem::novo(limites.max_linhas);
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
                let bruto = ler_valor(&mut leitor)?;
                if coluna >= limites.max_colunas {
                    montagem.aba.colunas_omitidas = true;
                    continue;
                }
                let estilo = tag.attr("s").and_then(|s| s.parse().ok()).unwrap_or(0);
                let celula = converter(tag.attr("t"), &bruto, estilos.formato(estilo), textos, data_1904);
                let texto = texto_seguro(&celula.texto, limites.max_celula_chars);
                let custo = texto.len().max(1);
                if custo > *orcamento {
                    montagem.aba.cortada = true;
                    break;
                }
                *orcamento -= custo;
                // A faixa de colunas inclui as células vazias (fórmula sem valor ocupa a coluna).
                linha.faixa = alarga(linha.faixa, coluna);
                if !texto.is_empty() {
                    linha.celulas.push((coluna, Celula { texto, numerica: celula.numerica }));
                }
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
/// `t` dentro dele formam o valor bruto. Célula sem `v` devolve vazio.
fn ler_valor(leitor: &mut Leitor) -> Result<String, String> {
    let mut valor = String::new();
    let (mut em_v, mut em_t) = (false, false);
    loop {
        let evento = leitor.proximo()?.ok_or_else(|| "fim inesperado do documento".to_string())?;
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
                    valor.push_str(&trecho);
                }
            }
            Evento::Fecha(nome) => match nome.as_str() {
                "c" => return Ok(valor),
                "v" => em_v = false,
                "t" => em_t = false,
                _ => {}
            },
        }
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
struct Montagem {
    aba: Aba,
    retidas: Vec<Vec<(usize, Celula)>>,
    menor: usize,
    maior: usize,
    max_linhas: usize,
}

impl Montagem {
    fn novo(max_linhas: usize) -> Self {
        Montagem { aba: Aba::default(), retidas: Vec::new(), menor: usize::MAX, maior: 0, max_linhas }
    }

    /// Fecha a linha: sem célula não vazia, descarta; senão conta e guarda até `max_linhas`.
    fn fechar(&mut self, linha: &mut Linha) {
        let Linha { celulas, faixa } = std::mem::take(linha);
        if celulas.is_empty() {
            return;
        }
        self.aba.linhas_total += 1;
        if self.retidas.len() >= self.max_linhas {
            return;
        }
        if let Some((menor, maior)) = faixa {
            self.menor = self.menor.min(menor);
            self.maior = self.maior.max(maior);
        }
        self.retidas.push(celulas);
    }

    fn finalizar(self) -> Aba {
        let Montagem { mut aba, retidas, menor, maior, .. } = self;
        if retidas.is_empty() {
            return aba;
        }
        let largura = maior - menor + 1;
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
