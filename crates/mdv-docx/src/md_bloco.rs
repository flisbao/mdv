//! Escreve um grupo de blocos (título, parágrafo, lista, código, citação, tabela) como Markdown.

use std::slice;

use crate::agrupar::Grupo;
use crate::escape;
use crate::limite::Limite;
use crate::md_inline::{inline, Modo};
use crate::md_notas::Notas;
use crate::md_tabela;
use crate::modelo::{Bloco, Trecho};

/// Escreve o grupo como Markdown, sem quebra de linha no fim.
///
/// Lista, citação e tabela descontam do `limite` a cada pedaço que produzem; título, parágrafo e
/// código saem de um bloco só, cujo tamanho a entrada já limita.
pub(crate) fn grupo(g: &Grupo, notas: &mut Notas, limite: &mut Limite) -> Result<String, String> {
    match g {
        Grupo::Lista(itens) => lista(itens, notas, limite),
        Grupo::Codigo(blocos) => Ok(codigo(blocos)),
        Grupo::Citacao(blocos) => citacao(blocos, notas, limite),
        Grupo::Bloco(bloco) => avulso(bloco, notas, limite),
    }
}

/// Escreve um bloco que veio sozinho; item, código e citação soltos valem como grupo de um.
fn avulso(bloco: &Bloco, notas: &mut Notas, limite: &mut Limite) -> Result<String, String> {
    Ok(match bloco {
        Bloco::Titulo { nivel, texto } => titulo(*nivel, texto, notas),
        Bloco::Paragrafo(texto) => paragrafo(texto, notas),
        Bloco::Tabela(t) => return md_tabela::tabela(t, notas, limite),
        Bloco::Item { .. } => return lista(slice::from_ref(bloco), notas, limite),
        Bloco::Codigo(_) => codigo(slice::from_ref(bloco)),
        Bloco::Citacao(_) => return citacao(slice::from_ref(bloco), notas, limite),
    })
}

/// Título ATX; o nível é limitado a 1..=6.
fn titulo(nivel: u8, texto: &[Trecho], notas: &mut Notas) -> String {
    let marcas = "#".repeat(nivel.clamp(1, 6) as usize);
    let mut corpo = inline(texto, notas, Modo::Titulo);
    // Um `#` final depois de espaço seria a sequência de fecho do título e sumiria.
    if corpo.ends_with('#') {
        corpo.insert(corpo.len() - 1, '\\');
    }
    format!("{marcas} {corpo}").trim_end().to_string()
}

fn paragrafo(texto: &[Trecho], notas: &mut Notas) -> String {
    inline(texto, notas, Modo::Bloco)
}

/// Itens colados, um por linha.
///
/// O nível do Word só diz quem é filho de quem: o pai de um item é o último item aberto de nível
/// menor, e sem pai ele fica na margem. Usar `4 × nível` direto faria uma lista que abre em
/// nível 1 começar com 4 espaços (bloco de código indentado) e um salto de nível virar texto do
/// item pai. O recuo do filho é o do pai mais a largura do marcador do pai, no mínimo 4 espaços:
/// serve sob `- ` e sob `10. `, e cresce sob `100. ` (5 colunas), em que 4 deixariam o filho
/// como continuação do texto. O item vai em `Modo::Linha` porque uma quebra `\` + nova linha
/// desfaria o recuo da lista.
fn lista(itens: &[Bloco], notas: &mut Notas, limite: &mut Limite) -> Result<String, String> {
    // Itens abertos: nível no Word e recuo dos filhos deles.
    let mut abertos: Vec<(u8, usize)> = Vec::new();
    let mut linhas: Vec<String> = Vec::new();
    for bloco in itens {
        let Bloco::Item { nivel, numerada, numero, texto } = bloco else { continue };
        // Com mais de 9 dígitos o CommonMark não reconhece a lista numerada.
        let marcador = if *numerada { format!("{}. ", (*numero).min(999_999_999)) } else { "- ".to_string() };
        while abertos.last().is_some_and(|&(n, _)| n >= *nivel) {
            abertos.pop();
        }
        let recuo = abertos.last().map_or(0, |&(_, r)| r);
        abertos.push((*nivel, recuo + marcador.len().max(4)));
        let linha = item(recuo, &marcador, texto, notas);
        limite.gastar(linha.len() + 1)?;
        linhas.push(linha);
    }
    Ok(linhas.join("\n"))
}

fn item(recuo: usize, marcador: &str, texto: &[Trecho], notas: &mut Notas) -> String {
    // O texto vem logo depois do marcador, onde `# `, `> ` ou `1. ` abririam outro bloco.
    let corpo = escape::inicio_de_linha(&inline(texto, notas, Modo::Linha));
    format!("{}{marcador}{corpo}", " ".repeat(recuo)).trim_end().to_string()
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
fn citacao(blocos: &[Bloco], notas: &mut Notas, limite: &mut Limite) -> Result<String, String> {
    let mut paragrafos: Vec<String> = Vec::new();
    for bloco in blocos {
        if let Bloco::Citacao(t) = bloco {
            let p = citado(t, notas);
            limite.gastar(p.len() + 3)?;
            paragrafos.push(p);
        }
    }
    Ok(paragrafos.join("\n>\n"))
}

/// Um parágrafo de citação com o prefixo em todas as linhas (linha vazia vira só `>`).
fn citado(texto: &[Trecho], notas: &mut Notas) -> String {
    let linhas: Vec<String> = inline(texto, notas, Modo::Bloco)
        .split('\n')
        .map(|l| if l.is_empty() { ">".to_string() } else { format!("> {l}") })
        .collect();
    linhas.join("\n")
}
