//! Escreve um grupo de blocos (título, parágrafo, lista, código, citação, tabela) como Markdown.

use std::slice;

use crate::agrupar::Grupo;
use crate::escape;
use crate::md_inline::{inline, Modo};
use crate::md_notas::Notas;
use crate::md_tabela;
use crate::modelo::{Bloco, Trecho};

/// Escreve o grupo como Markdown, sem quebra de linha no fim.
pub(crate) fn grupo(g: &Grupo, notas: &mut Notas) -> String {
    match g {
        Grupo::Lista(itens) => lista(itens, notas),
        Grupo::Codigo(blocos) => codigo(blocos),
        Grupo::Citacao(blocos) => citacao(blocos, notas),
        Grupo::Bloco(bloco) => avulso(bloco, notas),
    }
}

/// Escreve um bloco que veio sozinho; item, código e citação soltos valem como grupo de um.
fn avulso(bloco: &Bloco, notas: &mut Notas) -> String {
    match bloco {
        Bloco::Titulo { nivel, texto } => titulo(*nivel, texto, notas),
        Bloco::Paragrafo(texto) => paragrafo(texto, notas),
        Bloco::Tabela(t) => md_tabela::tabela(t, notas),
        Bloco::Item { .. } => lista(slice::from_ref(bloco), notas),
        Bloco::Codigo(_) => codigo(slice::from_ref(bloco)),
        Bloco::Citacao(_) => citacao(slice::from_ref(bloco), notas),
    }
}

/// Título ATX; o nível é limitado a 1..=6.
fn titulo(nivel: u8, texto: &[Trecho], notas: &mut Notas) -> String {
    let marcas = "#".repeat(nivel.clamp(1, 6) as usize);
    format!("{marcas} {}", inline(texto, notas, Modo::Titulo)).trim_end().to_string()
}

fn paragrafo(texto: &[Trecho], notas: &mut Notas) -> String {
    inline(texto, notas, Modo::Bloco)
}

/// Itens colados, um por linha.
///
/// O recuo é de 4 espaços por nível: serve tanto sob `- ` (2 colunas) quanto sob `10. `
/// (4 colunas), pois o filho precisa começar na coluna do texto do pai ou além. O item vai em
/// `Modo::Linha` porque uma quebra `\` + nova linha desfaria o recuo da lista.
fn lista(itens: &[Bloco], notas: &mut Notas) -> String {
    let linhas: Vec<String> = itens
        .iter()
        .filter_map(|bloco| match bloco {
            Bloco::Item { nivel, numerada, numero, texto } => {
                Some(item(*nivel, *numerada, *numero, texto, notas))
            }
            _ => None,
        })
        .collect();
    linhas.join("\n")
}

fn item(nivel: u8, numerada: bool, numero: u32, texto: &[Trecho], notas: &mut Notas) -> String {
    let recuo = "    ".repeat(nivel as usize);
    let marcador = if numerada { format!("{numero}. ") } else { "- ".to_string() };
    // O texto vem logo depois do marcador, onde `# `, `> ` ou `1. ` abririam outro bloco.
    let corpo = escape::inicio_de_linha(&inline(texto, notas, Modo::Linha));
    format!("{recuo}{marcador}{corpo}").trim_end().to_string()
}

/// Blocos de código seguidos numa cerca só, sem escape do conteúdo.
fn codigo(blocos: &[Bloco]) -> String {
    let linhas: Vec<&str> = blocos
        .iter()
        .filter_map(|b| if let Bloco::Codigo(s) = b { Some(s.as_str()) } else { None })
        .collect();
    let conteudo = linhas.join("\n");
    let cerca = cerca(&conteudo);
    format!("{cerca}\n{conteudo}\n{cerca}")
}

/// Cerca com ao menos 3 crases e uma a mais que a maior sequência de crases do conteúdo,
/// para que o conteúdo nunca feche a cerca antes da hora.
fn cerca(conteudo: &str) -> String {
    let mut maior = 0;
    let mut atual = 0;
    for c in conteudo.chars() {
        atual = if c == '`' { atual + 1 } else { 0 };
        maior = maior.max(atual);
    }
    "`".repeat((maior + 1).max(3))
}

/// Parágrafos de citação separados por uma linha `>`; cada linha leva o prefixo `> `.
fn citacao(blocos: &[Bloco], notas: &mut Notas) -> String {
    let paragrafos: Vec<String> = blocos
        .iter()
        .filter_map(|b| if let Bloco::Citacao(t) = b { Some(citado(t, notas)) } else { None })
        .collect();
    paragrafos.join("\n>\n")
}

/// Um parágrafo de citação com o prefixo em todas as linhas (linha vazia vira só `>`).
fn citado(texto: &[Trecho], notas: &mut Notas) -> String {
    let linhas: Vec<String> = inline(texto, notas, Modo::Bloco)
        .split('\n')
        .map(|l| if l.is_empty() { ">".to_string() } else { format!("> {l}") })
        .collect();
    linhas.join("\n")
}
