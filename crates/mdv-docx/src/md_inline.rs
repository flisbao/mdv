//! Trechos em linha para Markdown: marcas, links, imagens e referências de nota.

use std::rc::Rc;

use crate::escape;
use crate::md_link;
use crate::md_notas::Notas;
use crate::modelo::{Marcas, Trecho};

/// Soma máxima dos destinos de link escritos por uma chamada de `inline`. Cada trecho de marcas
/// diferentes repete o destino, então sem teto uma URL de 4 KiB em centenas de milhares de runs
/// viraria gigabytes de saída; o que passa disso fica como texto, sem link.
pub(crate) const MAX_LINKS_BYTES: usize = 64 * 1024;

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
    Grupo { texto: String, marcas: Marcas, link: Option<Rc<str>> },
    Pronta(String),
}

/// Converte os trechos em uma linha (ou parágrafo) de Markdown.
///
/// Runs vizinhos com as mesmas marcas e o mesmo link se fundem antes de escapar: o Word parte o
/// texto em vários runs sem motivo visual, e `**a****b**` não fecha direito. Os espaços das
/// bordas de um grupo ficam fora dos marcadores, pois `** a**` não vira negrito. O resultado é
/// aparado (4 espaços iniciais virariam bloco de código; espaços finais quebram o `\` de
/// quebra); só no modo `Bloco` o começo de cada linha é escapado, depois de aparar. Cada modo
/// muda a quebra, o escape de `|` (célula, inclusive no destino do link) e o negrito (título).
pub(crate) fn inline(trechos: &[Trecho], notas: &mut Notas, modo: Modo) -> String {
    let pecas = fundir(trechos, notas, modo);
    let mut links = 0;
    let juntos: String = pecas.iter().map(|p| escrever(p, modo, &mut links)).collect();
    let texto = aparar(&juntos, modo);
    if modo == Modo::Bloco {
        escapar_linhas(&texto)
    } else {
        texto
    }
}

/// Escapa o começo de cada linha de um parágrafo do modo `Bloco`.
///
/// Todo `\n` do resultado vem de um `Quebra` (as quebras do texto já viraram `Quebra` em
/// `segmentos`), e a linha que vem depois dele também pode parecer título, lista, citação ou
/// régua; por isso cada linha é aparada à esquerda (4 espaços virariam código) e escapada.
fn escapar_linhas(texto: &str) -> String {
    let linhas: Vec<String> = texto
        .split('\n')
        .map(|linha| escape::inicio_de_linha(linha.trim_start()))
        .collect();
    linhas.join("\n")
}

/// Reduz os trechos a peças, fundindo `Texto` vizinhos de mesmas marcas e mesmo link.
fn fundir(trechos: &[Trecho], notas: &mut Notas, modo: Modo) -> Vec<Peca> {
    let mut pecas: Vec<Peca> = Vec::new();
    for trecho in trechos {
        match trecho {
            Trecho::Texto { texto, marcas, link } => {
                for (i, segmento) in segmentos(texto, modo).into_iter().enumerate() {
                    if i > 0 {
                        pecas.push(Peca::Pronta(quebra(modo).to_string()));
                    }
                    acrescentar(&mut pecas, segmento, *marcas, link);
                }
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

/// Junta o texto ao grupo anterior se as marcas e o link forem os mesmos; senão abre um grupo.
fn acrescentar(pecas: &mut Vec<Peca>, texto: String, marcas: Marcas, link: &Option<Rc<str>>) {
    if let Some(Peca::Grupo { texto: acc, marcas: m, link: l }) = pecas.last_mut() {
        if *m == marcas && l == link {
            acc.push_str(&texto);
            return;
        }
    }
    pecas.push(Peca::Grupo { texto, marcas, link: link.clone() });
}

/// Neutraliza `\r`, `\n` e `\t` do texto (o `sanitize` os mantém) e o parte em linhas.
///
/// Uma quebra de linha crua forjaria título, lista ou citação no `Bloco` e sairia do contexto de
/// uma linha só nos outros modos (título, item, definição de nota, célula); por isso, fora do
/// `Bloco`, tudo vira espaço. No `Bloco` o `\n` vira uma quebra de linha de verdade (`Quebra`),
/// e `\r`/`\t` viram espaço. Vale também dentro de código, onde a quebra cairia fora da crase.
fn segmentos(texto: &str, modo: Modo) -> Vec<String> {
    let plano = texto.replace("\r\n", "\n").replace(['\r', '\t'], " ");
    if modo == Modo::Bloco {
        plano.split('\n').map(String::from).collect()
    } else {
        vec![plano.replace('\n', " ")]
    }
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
    // Mesma regra de `segmentos`: o alt não pode trazer quebra de linha, nem no `Bloco`.
    let plano = alt.replace(['\r', '\n', '\t'], " ");
    let alt = plano.trim();
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
/// `links` soma os bytes de destino já escritos (ver `MAX_LINKS_BYTES`).
fn escrever(peca: &Peca, modo: Modo, links: &mut usize) -> String {
    match peca {
        Peca::Pronta(s) => s.clone(),
        Peca::Grupo { texto, marcas, link } => grupo(texto, *marcas, link.as_deref(), modo, links),
    }
}

/// Escreve um grupo: espaços das bordas por fora, e por dentro marcadores e, se couber, link.
fn grupo(texto: &str, marcas: Marcas, link: Option<&str>, modo: Modo, links: &mut usize) -> String {
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
    if let Some(url) = link.filter(|u| md_link::permitido(u) && *links + u.len() <= MAX_LINKS_BYTES) {
        *links += url.len();
        // Em célula, `|` no destino dividiria a coluna da tabela GFM.
        let destino = md_link::destino(url);
        let destino = if modo == Modo::Celula { destino.replace('|', "%7C") } else { destino };
        corpo = format!("[{corpo}]({destino})");
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

/// Apara as bordas; no modo `Bloco` tira também as `\` soltas de quebras no fim.
///
/// Texto escapado dobra toda barra invertida, então um número ímpar de barras no fim só pode
/// ser o marcador de quebra de linha de um `Trecho::Quebra` final; repete-se porque várias
/// quebras seguidas deixam uma barra após a outra (`a\⏎\⏎`).
fn aparar(s: &str, modo: Modo) -> String {
    let mut texto = s.trim();
    if modo != Modo::Bloco {
        return texto.to_string();
    }
    loop {
        let barras = texto.bytes().rev().take_while(|&b| b == b'\\').count();
        if barras % 2 == 0 {
            return texto.to_string();
        }
        texto = texto[..texto.len() - 1].trim_end();
    }
}
