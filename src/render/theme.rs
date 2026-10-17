//! Cores e constantes visuais.

use ratatui::style::Color;

pub(super) const H1: Color = Color::Rgb(210, 168, 255);

pub(super) const H2: Color = Color::Rgb(121, 192, 255);

pub(super) const H3: Color = Color::Rgb(126, 231, 135);

pub(super) const H4: Color = Color::Rgb(230, 237, 243);

pub(crate) const DIM: Color = Color::Rgb(110, 118, 129);

pub(super) const LINK: Color = Color::Rgb(88, 166, 255);

pub(super) const INLINE_CODE_FG: Color = Color::Rgb(255, 166, 87);

pub(super) const INLINE_CODE_BG: Color = Color::Rgb(47, 52, 60);

pub(super) const CODE_BG: Color = Color::Rgb(40, 44, 52);

pub(super) const DIFF_ADD_FG: Color = Color::Rgb(126, 231, 135);

pub(super) const DIFF_ADD_BG: Color = Color::Rgb(35, 64, 45);

pub(super) const DIFF_DEL_FG: Color = Color::Rgb(255, 123, 114);

pub(super) const DIFF_DEL_BG: Color = Color::Rgb(74, 38, 42);

pub(super) const DIFF_HUNK_FG: Color = Color::Rgb(121, 192, 255);

pub(super) const QUOTE: Color = Color::Rgb(139, 148, 158);

pub(super) const BULLET: Color = Color::Rgb(126, 231, 135);

pub(super) const QUOTE_BAR_COLOR: Color = Color::Rgb(63, 185, 80);

/// Barra que prefixa cada linha de uma citação.
pub(super) const QUOTE_BAR: &str = "▎ ";
