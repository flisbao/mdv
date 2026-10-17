//! Peças de texto compartilhadas por `mdv` e `mdv-tools`. Sem I/O: só funções puras sobre `&str`.

mod link;
mod outline;
mod parse;
mod text;

pub use link::{is_confined, is_external, percent_decode, slugify};
pub use outline::{headings, section_text, Heading};
pub use parse::parse_options;
pub use text::sanitize;
