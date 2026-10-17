//! Modelo intermediário da exportação: o que o Markdown diz, sem largura de terminal nem estilo
//! de nenhum formato. `parse.rs` o produz; os escritores (PDF, DOCX, ODT) só o consomem.

use image::DynamicImage;
use pulldown_cmark::Alignment;
use std::sync::Arc;

/// Documento inteiro: título (do front matter, se houver) e blocos em ordem.
#[derive(Debug, Default)]
pub struct Document {
    pub title: Option<String>,
    pub blocks: Vec<Block>,
}

/// Trecho de texto com um único conjunto de estilos.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Run {
    pub text: String,
    pub bold: bool,
    pub italic: bool,
    pub strike: bool,
    pub code: bool,
    pub link: Option<String>,
}

/// Item de um parágrafo: texto estilizado ou quebra de linha forçada.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Inline {
    Run(Run),
    Break,
}

#[derive(Debug)]
pub struct ListItem {
    /// `Some(feito)` para itens de tarefa (`- [x]`).
    pub task: Option<bool>,
    pub blocks: Vec<Block>,
}

#[derive(Debug)]
pub enum Block {
    Heading { level: u8, text: Vec<Inline> },
    Paragraph(Vec<Inline>),
    List { ordered: bool, start: u64, items: Vec<ListItem> },
    /// Citação; `title` é o rótulo de um alerta GFM (`Nota`, `Aviso`...).
    Quote { title: Option<String>, blocks: Vec<Block> },
    Code { lang: String, text: String },
    Table { aligns: Vec<Alignment>, header: Vec<Vec<Inline>>, rows: Vec<Vec<Vec<Inline>>> },
    Rule,
    /// Imagem ainda não carregada; `images::resolve` a troca por `Picture` (ou por um parágrafo).
    Image { alt: String, src: String },
    /// Imagem já decodificada (arquivo local, `data:` ou diagrama Mermaid).
    Picture { alt: String, image: Arc<DynamicImage> },
}

impl Run {
    pub fn plain(text: impl Into<String>) -> Run {
        Run { text: text.into(), ..Run::default() }
    }

    pub fn bold(text: impl Into<String>) -> Run {
        Run { text: text.into(), bold: true, ..Run::default() }
    }
}

/// Texto puro de uma sequência de `Inline` (quebra vira espaço). Só os testes usam.
#[cfg(test)]
pub fn plain_text(inlines: &[Inline]) -> String {
    inlines
        .iter()
        .map(|i| match i {
            Inline::Run(r) => r.text.as_str(),
            Inline::Break => " ",
        })
        .collect()
}
