//! API pública: `.xlsx` → Markdown com uma tabela por aba visível. Orquestra as partes do pacote
//! (textos, estilos, abas) e aplica os tetos de abas, de bytes de célula e do Markdown final.

use crate::estilos::Estilos;
use crate::limites::Limites;
use crate::markdown::escrever_aba;
use crate::pacote::Pacote;
use crate::pasta::{self, AbaRef};
use crate::planilha;
use crate::textos;
use crate::{Budget, MAX_MARKDOWN_BYTES};

/// Converte com os tetos de produção (`Limites::default`).
pub fn to_markdown(bytes: &[u8], budget: &dyn Budget) -> Result<String, String> {
    converter(bytes, budget, &Limites::default())
}

/// Converte a planilha em Markdown: uma aba visível vira `# nome` + tabela. Abas ocultas não entram.
/// Passar de algum teto corta a saída com um aviso em itálico; só o formato errado ou o pacote
/// quebrado dão erro.
pub fn converter(bytes: &[u8], budget: &dyn Budget, limites: &Limites) -> Result<String, String> {
    let mut pacote = Pacote::abrir(bytes)?;
    let pasta = pasta::abrir(&mut pacote, budget)?;
    let textos = match pasta.textos.as_deref() {
        Some(nome) => match pacote.parte(nome)? {
            Some(xml) => textos::ler(&xml, limites, budget)?,
            None => Vec::new(),
        },
        None => Vec::new(),
    };
    let estilos = match pasta.estilos.as_deref() {
        Some(nome) => match pacote.parte(nome)? {
            Some(xml) => Estilos::carregar(&xml, budget)?,
            None => Estilos::vazio(),
        },
        None => Estilos::vazio(),
    };
    let visiveis: Vec<&AbaRef> = pasta.abas.iter().filter(|a| !a.oculta).collect();
    if visiveis.is_empty() {
        return Ok("_(a planilha não tem abas visíveis)_\n".to_string());
    }
    let mostradas = visiveis.len().min(limites.max_abas);
    let mut saida = String::new();
    let mut orcamento = limites.max_bytes_celulas;
    for aba_ref in &visiveis[..mostradas] {
        budget.check()?;
        let xml = pacote.parte(&aba_ref.parte)?.unwrap_or_default();
        let aba = planilha::ler(&xml, &textos, &estilos, pasta.data_1904, limites, &mut orcamento, budget)?;
        escrever_aba(&aba_ref.nome, &aba, limites, &mut saida);
        if saida.len() > MAX_MARKDOWN_BYTES {
            return Err("planilha convertida grande demais".to_string());
        }
    }
    if visiveis.len() > mostradas {
        saida.push_str(&format!("_(planilha cortada: {mostradas} de {} abas mostradas)_\n", visiveis.len()));
    }
    Ok(saida)
}
