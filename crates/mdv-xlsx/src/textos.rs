//! `sharedStrings.xml` → `Vec<String>`: um texto por `<si>`, com tetos de quantidade e de tamanho.
//! Texto rico (`<r>`) junta os `<t>`; o fonético (`<rPh>`) é pulado. Passou de `max_textos`, o
//! restante não é guardado: `s` com índice fora da lista já vira célula vazia.
//! Os textos saem limitados a `max_celula_chars + 1` caracteres; o sanitize final e o corte (`…`)
//! ficam com `planilha::ler`, que é o único ponto que aplica `texto_seguro`.

use crate::texto_limitado::TextoLimitado;
use crate::limites::Limites;
use crate::xml_leitor::{Evento, Leitor};
use crate::Budget;

pub(crate) fn ler(xml: &[u8], limites: &Limites, budget: &dyn Budget) -> Result<Vec<String>, String> {
    let mut leitor = Leitor::novo(xml, budget);
    let mut textos = Vec::new();
    // `atual` só existe enquanto há vaga em `textos`: passou do teto, o `<si>` é lido e descartado.
    let mut atual: Option<TextoLimitado> = None;
    let mut em_t = false;
    while let Some(evento) = leitor.proximo()? {
        match evento {
            Evento::Abre(tag) => match tag.nome.as_str() {
                "si" => atual = (textos.len() < limites.max_textos).then(|| TextoLimitado::novo(limites.max_celula_chars)),
                "rPh" => leitor.pular()?,
                "t" => em_t = true,
                _ => {}
            },
            Evento::Texto(trecho) if em_t => {
                if let Some(texto) = atual.as_mut() {
                    texto.acrescentar(&trecho);
                }
            }
            Evento::Fecha(nome) => match nome.as_str() {
                "t" => em_t = false,
                "si" => {
                    if let Some(texto) = atual.take() {
                        textos.push(texto.into_bruto());
                    }
                }
                _ => {}
            },
            _ => {}
        }
    }
    Ok(textos)
}
