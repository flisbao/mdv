//! API pública: `.xlsx` → Markdown com uma tabela por aba visível. Orquestra as partes do pacote
//! (textos, estilos, abas) e aplica os tetos de abas, de bytes de célula e do Markdown final.

use crate::estilos::Estilos;
use crate::limites::{Limites, Orcamento};
use crate::markdown::{escrever_aba, escrever_abas_cortadas, escrever_ausente, escrever_markdown_cortado, escrever_sem_abas_visiveis};
use crate::pacote::Pacote;
use crate::pasta::{self, AbaRef};
use crate::planilha::{self, ContextoPasta};
use crate::textos;
use crate::Budget;

/// Converte com os tetos de produção (`Limites::default`).
pub fn to_markdown(bytes: &[u8], budget: &dyn Budget) -> Result<String, String> {
    to_markdown_com_limites(bytes, budget, &Limites::default())
}

/// Converte a planilha em Markdown: uma aba visível vira `# nome` + tabela. Abas ocultas não entram.
/// Passar de algum teto (abas, células, bytes de célula ou `max_markdown_bytes`) corta a saída
/// com um aviso em itálico; só o formato errado ou o pacote (ZIP/XML) quebrado dão erro.
pub(crate) fn to_markdown_com_limites(bytes: &[u8], budget: &dyn Budget, limites: &Limites) -> Result<String, String> {
    let mut pacote = Pacote::abrir(bytes)?;
    let pasta = pasta::abrir(&mut pacote, budget)?;
    let textos = ler_parte_opcional(&mut pacote, pasta.textos.as_deref(), Vec::new, |xml| textos::ler(xml, limites, budget))?;
    let estilos = ler_parte_opcional(&mut pacote, pasta.estilos.as_deref(), Estilos::vazio, |xml| Estilos::carregar(xml, budget))?;
    let visiveis: Vec<&AbaRef> = pasta.abas.iter().filter(|a| !a.oculta).collect();
    if visiveis.is_empty() {
        let mut saida = String::new();
        escrever_sem_abas_visiveis(&mut saida);
        return Ok(saida);
    }
    let mostradas = visiveis.len().min(limites.max_abas);
    let mut saida = String::new();
    let mut orcamento = Orcamento::novo(limites.max_bytes_celulas);
    let ctx = ContextoPasta { textos: &textos, estilos: &estilos, data_1904: pasta.data_1904, limites };
    for aba_ref in &visiveis[..mostradas] {
        budget.check()?;
        // Cada aba é escrita à parte: só entra na saída se couber no teto de Markdown.
        let mut bloco = String::new();
        match pacote.parte(&aba_ref.parte)? {
            Some(xml) => {
                let aba = planilha::ler(&xml, &ctx, &mut orcamento, budget)?;
                escrever_aba(&aba_ref.nome, &aba, &mut bloco);
            }
            None => escrever_ausente(&aba_ref.nome, &mut bloco),
        }
        if saida.len() + bloco.len() > limites.max_markdown_bytes {
            // Se nem a primeira aba cabe, a saída é só o aviso (sem o título dela).
            escrever_markdown_cortado(limites.max_markdown_bytes, &mut saida);
            return Ok(saida);
        }
        saida.push_str(&bloco);
    }
    if visiveis.len() > mostradas {
        escrever_abas_cortadas(mostradas, visiveis.len(), &mut saida);
    }
    Ok(saida)
}

/// Lê uma parte que a pasta pode não declarar ou que pode faltar no pacote: nos dois casos
/// o resultado é `vazio()`, não erro.
fn ler_parte_opcional<T>(
    pacote: &mut Pacote,
    nome: Option<&str>,
    vazio: impl FnOnce() -> T,
    ler: impl FnOnce(&[u8]) -> Result<T, String>,
) -> Result<T, String> {
    match nome {
        Some(nome) => match pacote.parte(nome)? {
            Some(xml) => ler(&xml),
            None => Ok(vazio()),
        },
        None => Ok(vazio()),
    }
}
