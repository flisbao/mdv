//! Montagem final: junta os grupos de blocos, as definições de notas e aplica o teto de tamanho.

use crate::agrupar::agrupar;
use crate::limite::{Limite, ERRO_GRANDE};
use crate::md_bloco::grupo;
use crate::md_notas::Notas;
use crate::modelo::Documento;
use crate::{Budget, MAX_MARKDOWN_BYTES};

/// Escreve o documento como Markdown, terminado por um único `\n`.
pub(crate) fn escrever(doc: &Documento, budget: &dyn Budget) -> Result<String, String> {
    escrever_com_teto(doc, budget, MAX_MARKDOWN_BYTES)
}

fn escrever_com_teto(doc: &Documento, budget: &dyn Budget, teto: usize) -> Result<String, String> {
    let mut notas = Notas::novo();
    let mut saida = String::new();
    for g in agrupar(&doc.blocos) {
        budget.check()?;
        let texto = grupo(&g, &mut notas, &mut Limite::novo(budget, teto))?;
        if texto.is_empty() {
            continue;
        }
        acrescentar(&mut saida, &texto, teto)?;
    }
    // As definições vêm depois do corpo porque só ele diz quais notas foram citadas e em que ordem.
    let definicoes = notas.definicoes(&doc.notas, &mut Limite::novo(budget, teto))?;
    if !definicoes.is_empty() {
        acrescentar(&mut saida, &definicoes, teto)?;
    }
    if saida.is_empty() {
        return Ok("*(documento sem texto)*\n".to_string());
    }
    saida.push('\n');
    Ok(saida)
}

/// Junta `texto` ao fim com uma linha em branco no meio, recusando passar do teto.
fn acrescentar(saida: &mut String, texto: &str, teto: usize) -> Result<(), String> {
    let separador = if saida.is_empty() { 0 } else { 2 };
    if saida.len() + separador + texto.len() > teto {
        return Err(ERRO_GRANDE.into());
    }
    if separador > 0 {
        saida.push_str("\n\n");
    }
    saida.push_str(texto);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modelo::{Bloco, Marcas, Trecho};
    use crate::Unlimited;
    use std::collections::HashMap;

    fn paragrafo(s: &str) -> Bloco {
        Bloco::Paragrafo(vec![Trecho::Texto { texto: s.into(), marcas: Marcas::default(), link: None }])
    }

    fn documento(blocos: Vec<Bloco>) -> Documento {
        Documento { blocos, notas: HashMap::new() }
    }

    #[test]
    fn teto_reduzido_recusa_o_texto() {
        let doc = documento(vec![paragrafo("um texto bem mais longo que dez bytes")]);
        let erro = escrever_com_teto(&doc, &Unlimited, 10).expect_err("erro");
        assert!(erro.contains("grande demais"), "{erro}");
    }

    #[test]
    fn texto_no_teto_passa() {
        let doc = documento(vec![paragrafo("abc")]);
        assert_eq!(escrever_com_teto(&doc, &Unlimited, 3).as_deref(), Ok("abc\n"));
    }

    #[test]
    fn blocos_ficam_separados_por_linha_em_branco() {
        let doc = documento(vec![paragrafo("a"), paragrafo("b")]);
        assert_eq!(escrever(&doc, &Unlimited).as_deref(), Ok("a\n\nb\n"));
    }

    #[test]
    fn paragrafo_vazio_nao_deixa_linhas_em_branco_extras() {
        let doc = documento(vec![paragrafo("a"), Bloco::Paragrafo(vec![]), paragrafo("b")]);
        assert_eq!(escrever(&doc, &Unlimited).as_deref(), Ok("a\n\nb\n"));
    }

    #[test]
    fn sem_blocos_avisa() {
        assert_eq!(escrever(&documento(vec![]), &Unlimited).as_deref(), Ok("*(documento sem texto)*\n"));
    }
}
