//! `sharedStrings.xml` → `Vec<String>`: um texto por `<si>`, com tetos de quantidade e de tamanho.
//! Texto rico (`<r>`) junta os `<t>`; o fonético (`<rPh>`) é pulado. Passou de `max_textos`, o
//! restante não é guardado: `s` com índice fora da lista já vira célula vazia.

use crate::celula::{acumular_limitado, texto_seguro};
use crate::limites::Limites;
use crate::xml_leitor::{Evento, Leitor};
use crate::Budget;

pub(crate) fn ler(xml: &[u8], limites: &Limites, budget: &dyn Budget) -> Result<Vec<String>, String> {
    let mut leitor = Leitor::novo(xml, budget);
    let mut textos = Vec::new();
    // `atual` só existe enquanto há vaga em `textos`: passou do teto, o `<si>` é lido e descartado.
    let mut atual: Option<String> = None;
    let mut em_t = false;
    while let Some(evento) = leitor.proximo()? {
        match evento {
            Evento::Abre(tag) => match tag.nome.as_str() {
                "si" => atual = (textos.len() < limites.max_textos).then(String::new),
                "rPh" => leitor.pular()?,
                "t" => em_t = true,
                _ => {}
            },
            Evento::Texto(trecho) if em_t => {
                if let Some(texto) = atual.as_mut() {
                    acumular_limitado(texto, &trecho, limites.max_celula_chars);
                }
            }
            Evento::Fecha(nome) => match nome.as_str() {
                "t" => em_t = false,
                "si" => {
                    if let Some(bruto) = atual.take() {
                        textos.push(texto_seguro(&bruto, limites.max_celula_chars));
                    }
                }
                _ => {}
            },
            _ => {}
        }
    }
    Ok(textos)
}
