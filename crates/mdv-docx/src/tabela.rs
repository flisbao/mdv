//! Leitura de tabela (`w:tbl`): linhas, células, mesclagens (`gridSpan`, `vMerge`) e tabelas aninhadas.

use crate::contexto::Contexto;
use crate::modelo::{Bloco, Celula, Tabela};
use crate::paragrafo::ler_paragrafo;
use crate::xml_leitor::{Evento, Leitor};
use crate::MAX_COLUNAS;

/// Lê um `w:tbl` (chamada logo depois do `Abre("tbl")`) e consome até o `Fecha` dele.
/// `tblPr` e `tblGrid` só descrevem aparência e largura, então são pulados.
pub(crate) fn ler_tabela(l: &mut Leitor, ctx: &mut Contexto) -> Result<Tabela, String> {
    let mut linhas = Vec::new();
    loop {
        match l.proximo()? {
            None => return Err("fim inesperado do documento".into()),
            Some(Evento::Fecha(n)) if n == "tbl" => return Ok(Tabela { linhas }),
            Some(Evento::Abre(tag)) => match tag.nome.as_str() {
                "tr" => {
                    ctx.budget.check()?;
                    linhas.push(ler_linha(l, ctx)?);
                }
                "tblPr" | "tblGrid" | "Fallback" => l.pular()?,
                _ => {}
            },
            Some(_) => {}
        }
    }
}

/// Lê um `w:tr` (depois do `Abre`) e devolve as células; linhas com números diferentes de células são aceitas.
fn ler_linha(l: &mut Leitor, ctx: &mut Contexto) -> Result<Vec<Celula>, String> {
    let mut celulas = Vec::new();
    loop {
        match l.proximo()? {
            None => return Err("fim inesperado do documento".into()),
            Some(Evento::Fecha(n)) if n == "tr" => return Ok(celulas),
            Some(Evento::Abre(tag)) => match tag.nome.as_str() {
                "tc" => celulas.push(ler_celula(l, ctx)?),
                "trPr" | "tblPrEx" | "Fallback" => l.pular()?,
                _ => {}
            },
            Some(_) => {}
        }
    }
}

/// Lê um `w:tc` (depois do `Abre`). O GFM não tem tabela dentro de tabela, então uma `w:tbl`
/// aninhada é achatada: seus parágrafos entram na célula de fora, em ordem. A recursão é limitada
/// pela profundidade máxima do `Leitor`. `gridSpan` é limitado a `MAX_COLUNAS` para um valor
/// absurdo não gerar milhares de colunas na saída; `vMerge` sem `val` ou diferente de `restart`
/// marca a continuação de uma mesclagem vertical. Outros elementos (`sdt`...) são transparentes.
fn ler_celula(l: &mut Leitor, ctx: &mut Contexto) -> Result<Celula, String> {
    let antes = ctx.contador.parar(true);
    let celula = ler_conteudo(l, ctx);
    ctx.contador.parar(antes);
    celula
}

/// Lê o conteúdo da célula até o `Fecha("tc")`; `ler_celula` cuida de parar a numeração.
fn ler_conteudo(l: &mut Leitor, ctx: &mut Contexto) -> Result<Celula, String> {
    let mut celula = Celula { paragrafos: Vec::new(), colunas: 1, continua: false };
    loop {
        match l.proximo()? {
            None => return Err("fim inesperado do documento".into()),
            Some(Evento::Fecha(n)) if n == "tc" => return Ok(celula),
            Some(Evento::Abre(tag)) => match tag.nome.as_str() {
                "p" => {
                    if let Some(bloco) = ler_paragrafo(l, ctx)? {
                        celula.paragrafos.extend(bloco.paragrafos());
                    }
                }
                "tbl" => {
                    let interna = ler_tabela(l, ctx)?;
                    celula.paragrafos.extend(Bloco::Tabela(interna).paragrafos());
                }
                "tcPr" => ler_tc(l, &mut celula)?,
                "Fallback" => l.pular()?,
                _ => {}
            },
            Some(_) => {}
        }
    }
}

/// Lê o `w:tcPr` (depois do `Abre`): só `gridSpan` e `vMerge` interessam; o resto é pulado.
fn ler_tc(l: &mut Leitor, celula: &mut Celula) -> Result<(), String> {
    loop {
        match l.proximo()? {
            None => return Err("fim inesperado do documento".into()),
            Some(Evento::Fecha(n)) if n == "tcPr" => return Ok(()),
            Some(Evento::Abre(tag)) => match tag.nome.as_str() {
                "gridSpan" => {
                    let n = tag.val().and_then(|v| v.trim().parse::<usize>().ok());
                    celula.colunas = n.unwrap_or(1).clamp(1, MAX_COLUNAS);
                }
                "vMerge" => celula.continua = tag.val() != Some("restart"),
                _ => l.pular()?,
            },
            Some(_) => {}
        }
    }
}

/// Lê o primeiro `w:tbl` do XML; ajuda os testes de tabela.
#[cfg(test)]
pub(crate) fn ler_xml(ctx: &mut Contexto, corpo: &str) -> Tabela {
    use crate::testutil::NS;
    use crate::Unlimited;
    let xml = format!("<w:tbl {NS}>{corpo}</w:tbl>");
    let mut l = Leitor::novo(xml.as_bytes(), &Unlimited);
    l.ate("tbl").unwrap().unwrap();
    let tabela = ler_tabela(&mut l, ctx).unwrap();
    assert_eq!(l.proximo().unwrap(), None, "a tabela deve ser consumida por inteiro");
    tabela
}
