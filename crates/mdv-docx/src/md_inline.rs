//! Trechos em linha para Markdown: marcas, links, imagens e referências de nota.

use crate::escape;
use crate::md_link;
use crate::md_notas::Notas;
use crate::modelo::{Marcas, Trecho};

/// Onde o texto em linha vai parar; muda quebras, escapes e o uso de negrito.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Modo {
    /// Parágrafo: quebra vira `\` + nova linha e o começo é escapado.
    Bloco,
    /// Linha única (item de lista, nota): quebra vira espaço.
    Linha,
    /// Célula de tabela: quebra vira `<br>` e `|` é escapado.
    Celula,
    /// Título: como `Linha`, sem negrito (o título já é forte).
    Titulo,
}

/// Peça já reduzida: um grupo de texto ainda por escapar ou um trecho pronto (quebra, imagem, nota).
enum Peca {
    Grupo { texto: String, marcas: Marcas, link: Option<String> },
    Pronta(String),
}

/// Converte os trechos em uma linha (ou parágrafo) de Markdown.
///
/// Runs vizinhos com as mesmas marcas e o mesmo link se fundem antes de escapar: o Word parte o
/// texto em vários runs sem motivo visual, e `**a****b**` não fecha direito. Os espaços das
/// bordas de um grupo ficam fora dos marcadores, pois `** a**` não vira negrito. O resultado é
/// aparado (4 espaços iniciais virariam bloco de código; espaços finais quebram o `\` de
/// quebra); só no modo `Bloco` o começo é escapado, depois de aparar. Cada modo muda a quebra,
/// o escape de `|` (célula) e o negrito (título).
pub(crate) fn inline(trechos: &[Trecho], notas: &mut Notas, modo: Modo) -> String {
    let pecas = fundir(trechos, notas, modo);
    let juntos: String = pecas.iter().map(|p| escrever(p, modo)).collect();
    let texto = aparar(&juntos, modo);
    if modo == Modo::Bloco {
        escape::inicio_de_linha(&texto)
    } else {
        texto
    }
}

/// Reduz os trechos a peças, fundindo `Texto` vizinhos de mesmas marcas e mesmo link.
fn fundir(trechos: &[Trecho], notas: &mut Notas, modo: Modo) -> Vec<Peca> {
    let mut pecas: Vec<Peca> = Vec::new();
    for trecho in trechos {
        match trecho {
            Trecho::Texto { texto, marcas, link } => {
                if let Some(Peca::Grupo { texto: acc, marcas: m, link: l }) = pecas.last_mut() {
                    if m == marcas && l == link {
                        acc.push_str(texto);
                        continue;
                    }
                }
                let grupo = Peca::Grupo { texto: texto.clone(), marcas: *marcas, link: link.clone() };
                pecas.push(grupo);
            }
            Trecho::Quebra => pecas.push(Peca::Pronta(quebra(modo).to_string())),
            Trecho::NotaRef(chave) => {
                pecas.push(Peca::Pronta(format!("[^{}]", notas.referenciar(chave))));
            }
            Trecho::Imagem(alt) => pecas.push(Peca::Pronta(imagem(alt, modo))),
        }
    }
    pecas
}

/// Marca de quebra de linha de cada modo.
fn quebra(modo: Modo) -> &'static str {
    match modo {
        Modo::Bloco => "\\\n",
        Modo::Celula => "<br>",
        Modo::Linha | Modo::Titulo => " ",
    }
}

/// Imagem vira uma linha em itálico com o texto alternativo; a imagem em si não é extraída.
fn imagem(alt: &str, modo: Modo) -> String {
    let alt = alt.trim();
    if alt.is_empty() {
        "*[imagem]*".to_string()
    } else {
        format!("*[imagem: {}]*", escapar(alt, modo))
    }
}

/// Escapa texto comum conforme o modo (célula também escapa `|` e troca quebras por espaço).
fn escapar(texto: &str, modo: Modo) -> String {
    if modo == Modo::Celula {
        escape::celula(texto)
    } else {
        escape::inline(texto)
    }
}

/// Escreve uma peça: peça pronta sai como está, grupo recebe escape, marcadores e link.
fn escrever(peca: &Peca, modo: Modo) -> String {
    match peca {
        Peca::Pronta(s) => s.clone(),
        Peca::Grupo { texto, marcas, link } => grupo(texto, *marcas, link.as_deref(), modo),
    }
}

/// Escreve um grupo: espaços das bordas por fora, e por dentro marcadores e, se couber, link.
fn grupo(texto: &str, marcas: Marcas, link: Option<&str>, modo: Modo) -> String {
    let miolo = texto.trim();
    if miolo.is_empty() {
        return escapar(texto, modo);
    }
    let inicio = &texto[..texto.len() - texto.trim_start().len()];
    let fim = &texto[texto.trim_end().len()..];
    let mut corpo = if marcas.codigo {
        codigo(miolo, modo)
    } else {
        marcar(&escapar(miolo, modo), marcas, modo)
    };
    if let Some(url) = link.filter(|u| md_link::permitido(u)) {
        corpo = format!("[{corpo}]({})", md_link::destino(url));
    }
    format!("{}{corpo}{}", escapar(inicio, modo), escapar(fim, modo))
}

/// Embrulha o texto já escapado: `~~` por fora, depois `**`, depois `*` (negrito some no título).
fn marcar(texto: &str, marcas: Marcas, modo: Modo) -> String {
    let mut saida = texto.to_string();
    if marcas.italico {
        saida = format!("*{saida}*");
    }
    if marcas.negrito && modo != Modo::Titulo {
        saida = format!("**{saida}**");
    }
    if marcas.tachado {
        saida = format!("~~{saida}~~");
    }
    saida
}

/// Trecho de código: sem escape, e em célula `|` precisa de barra mesmo dentro das crases,
/// senão a tabela GFM quebra a coluna.
fn codigo(texto: &str, modo: Modo) -> String {
    if modo == Modo::Celula {
        cerca(&texto.replace(['\n', '\r', '\t'], " ").replace('|', "\\|"))
    } else {
        cerca(texto)
    }
}

/// Cerca de crases para um trecho de código.
///
/// A cerca tem uma crase a mais que a maior sequência de crases do texto, para o texto não
/// fechá-la antes da hora; havendo crase no texto, um espaço de guarda de cada lado impede que
/// uma crase da borda cole na cerca (o leitor Markdown descarta esse par de espaços).
fn cerca(texto: &str) -> String {
    let mut maior = 0;
    let mut atual = 0;
    for c in texto.chars() {
        atual = if c == '`' { atual + 1 } else { 0 };
        maior = maior.max(atual);
    }
    let cerca = "`".repeat(maior + 1);
    let guarda = if maior > 0 { " " } else { "" };
    format!("{cerca}{guarda}{texto}{guarda}{cerca}")
}

/// Apara as bordas; no modo `Bloco` tira também a `\` solta de uma quebra no fim.
///
/// Texto escapado dobra toda barra invertida, então um número ímpar de barras no fim só pode
/// ser o marcador de quebra de linha de um `Trecho::Quebra` final.
fn aparar(s: &str, modo: Modo) -> String {
    let mut texto = s.trim();
    if modo == Modo::Bloco {
        let barras = texto.bytes().rev().take_while(|&b| b == b'\\').count();
        if barras % 2 == 1 {
            texto = texto[..texto.len() - 1].trim_end();
        }
    }
    texto.to_string()
}
