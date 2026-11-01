//! `sharedStrings.xml` → `Vec<String>`: um texto por `<si>`, com tetos de quantidade e de tamanho.
//! Texto rico (`<r>`) junta os `<t>`; o fonético (`<rPh>`) é pulado. Passou de `max_textos`, o
//! restante entra vazio, para que os índices de `s` continuem válidos.

use crate::celula::texto_seguro;
use crate::limites::Limites;
use crate::xml_leitor::{Evento, Leitor};
use crate::Budget;

pub(crate) fn ler(xml: &[u8], limites: &Limites, budget: &dyn Budget) -> Result<Vec<String>, String> {
    let mut leitor = Leitor::novo(xml, budget);
    let mut textos = Vec::new();
    let mut atual: Option<String> = None;
    let mut em_t = false;
    while let Some(evento) = leitor.proximo()? {
        match evento {
            Evento::Abre(tag) => match tag.nome.as_str() {
                "si" => atual = Some(String::new()),
                "rPh" => leitor.pular()?,
                "t" => em_t = true,
                _ => {}
            },
            Evento::Texto(trecho) if em_t => {
                if let Some(texto) = atual.as_mut() {
                    texto.push_str(&trecho);
                }
            }
            Evento::Fecha(nome) => match nome.as_str() {
                "t" => em_t = false,
                "si" => {
                    let bruto = atual.take().unwrap_or_default();
                    let guardar = textos.len() < limites.max_textos;
                    textos.push(if guardar { texto_seguro(&bruto, limites.max_celula_chars) } else { String::new() });
                }
                _ => {}
            },
            _ => {}
        }
    }
    Ok(textos)
}
