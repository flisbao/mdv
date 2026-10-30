//! Leitura de HTML para o terminal: HTML + CSS mínimo viram células coloridas.
//! Biblioteca pura: sem `ratatui`, sem I/O e sem rede.

mod boxes;
pub mod color;
pub mod css;
pub mod ctx;
mod deck;
pub mod flow;
pub mod guard;
pub mod index;
mod flex;
mod form;
mod flex_size;
mod inline;
mod list;
mod media;
mod pre;
pub mod length;
mod render;
pub mod model;
pub mod selector;
pub mod style;
mod style_apply;
mod style_flex;
pub mod text;
#[cfg(test)]
mod testutil;
mod table;
mod script;
mod ua;

pub use guard::Limits;
pub use model::{Cell, Document, Face, Heading, Line, Page, Rgb};

/// Opções de leitura: largura das linhas, largura do viewport para `@media` e tetos.
#[derive(Clone, Debug)]
pub struct Options {
    /// Colunas de cada linha (limitado a 20 até 400).
    pub width: usize,
    /// Largura do viewport em px para `@media`; 0 usa `width * 8`.
    pub viewport_px: u32,
    /// Tetos de tamanho, profundidade e memória.
    pub limits: Limits,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            width: 80,
            viewport_px: 0,
            limits: Limits::default(),
        }
    }
}

/// Lê um HTML e devolve as páginas de células. Sem JavaScript e sem rede.
pub fn render(html: &str, opts: &Options, budget: &dyn Budget) -> Result<Document, String> {
    let largura = opts.width.clamp(20, 400);
    let viewport = if opts.viewport_px == 0 {
        u32::try_from(largura).unwrap_or(400).saturating_mul(8)
    } else {
        opts.viewport_px
    };
    render::render(html, largura, viewport, &opts.limits, budget)
}

/// Orçamento de memória e tempo consultado pelos laços de leitura.
pub trait Budget {
    fn check(&self) -> Result<(), String>;
}

/// Orçamento que nunca recusa (uso em testes e em chamadas sem teto externo).
pub struct Unlimited;

impl Budget for Unlimited {
    fn check(&self) -> Result<(), String> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn tamanho_dos_arquivos() {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/src");
        let mut grandes = Vec::new();
        for entrada in std::fs::read_dir(dir).unwrap() {
            let caminho = entrada.unwrap().path();
            if caminho.extension().is_none_or(|e| e != "rs") {
                continue;
            }
            let texto = std::fs::read_to_string(&caminho).unwrap();
            let producao = texto.split("#[cfg(test)]").next().unwrap_or("");
            let linhas = producao.lines().count();
            if linhas > 500 {
                grandes.push(format!("{}: {linhas}", caminho.display()));
            }
        }
        assert!(grandes.is_empty(), "arquivos acima de 500 linhas: {grandes:?}");
    }
}
