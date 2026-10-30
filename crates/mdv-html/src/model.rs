//! Modelo de saída: células coloridas em linhas, páginas e o documento inteiro.
//! Não depende de `ratatui`; o `mdv` converte estas células no adaptador `src/html.rs`.

use unicode_width::UnicodeWidthChar;

/// Cor RGB de 8 bits por canal.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rgb(pub u8, pub u8, pub u8);

/// Estilo de uma célula. `link` é o índice em `Document::links`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Face {
    pub fg: Option<Rgb>,
    pub bg: Option<Rgb>,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub dim: bool,
    pub strike: bool,
    pub link: Option<u32>,
}

/// Uma célula: um caractere com seu estilo.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cell {
    pub ch: char,
    pub face: Face,
}

/// Uma linha de células já quebrada na largura pedida.
pub type Line = Vec<Cell>;

/// Largura em colunas de terminal de uma linha (emoji e CJK contam 2).
pub fn line_width(l: &[Cell]) -> usize {
    l.iter()
        .map(|c| UnicodeWidthChar::width(c.ch).unwrap_or(0))
        .sum()
}

/// Uma página (apresentação) ou o corpo inteiro quando não há fatiamento.
#[derive(Clone, Debug, PartialEq)]
pub struct Page {
    pub title: Option<String>,
    pub lines: Vec<Line>,
}

/// Título de seção: página, linha dentro da página, nível (1 a 6) e texto.
#[derive(Clone, Debug, PartialEq)]
pub struct Heading {
    pub page: usize,
    pub line: usize,
    pub level: u8,
    pub text: String,
}

/// Documento HTML já lido: páginas, links (URLs) e títulos.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Document {
    pub title: Option<String>,
    pub pages: Vec<Page>,
    pub links: Vec<String>,
    pub headings: Vec<Heading>,
}
