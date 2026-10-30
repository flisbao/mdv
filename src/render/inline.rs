//! Texto em linha: peças estilizadas, buffer e marcas de link.

use super::*;

/// Item do texto em linha: trecho estilizado ou quebra de linha forçada.
#[derive(Clone)]
pub(super) enum Piece {
    Text(String, Style),
    /// Código em linha: os espaços são literais (não colapsam nem quebram a linha).
    Code(String, Style),
    Break,
}

impl Piece {
    /// Texto e estilo para contextos sem quebra de linha (células de tabela): `Break` vira espaço.
    pub(super) fn as_text(&self) -> (&str, Style) {
        match self {
            Piece::Text(t, st) | Piece::Code(t, st) => (t, *st),
            Piece::Break => (" ", Style::default()),
        }
    }

    pub(super) fn width(&self) -> usize {
        self.as_text().0.width()
    }
}

pub fn link_mark(id: usize) -> Color {
    let id = u16::try_from(id).unwrap_or(u16::MAX);
    Color::Rgb(LINK_MARK_R, (id >> 8) as u8, (id & 0xFF) as u8)
}

/// Índice (em `Rendered.links`) do link a que o estilo pertence, se for texto de link.
pub fn link_id(style: &Style) -> Option<usize> {
    match style.underline_color {
        Some(Color::Rgb(LINK_MARK_R, hi, lo)) => Some((usize::from(hi) << 8) | usize::from(lo)),
        _ => None,
    }
}

/// Tira a marca de link da linha (a marca só serve à navegação, não deve chegar ao terminal).
pub fn strip_link_marks(line: &mut Line<'static>) {
    for sp in &mut line.spans {
        if link_id(&sp.style).is_some() {
            sp.style.underline_color = None;
        }
    }
}

/// Texto em linha ainda não quebrado, com as pilhas de estilo e de destinos de link.
#[derive(Default)]
pub(super) struct InlineBuffer {
    pub(super) pieces: Vec<Piece>,
    pub(super) styles: Vec<Style>,
    pub(super) links: Vec<String>,
}

impl InlineBuffer {
    pub(super) fn style(&self) -> Style {
        self.styles.iter().fold(Style::default(), |a, s| a.patch(*s))
    }

    pub(super) fn push_style(&mut self, style: Style) {
        self.styles.push(style);
    }

    pub(super) fn pop_style(&mut self) {
        self.styles.pop();
    }

    pub(super) fn push(&mut self, piece: Piece) {
        self.pieces.push(piece);
    }

    pub(super) fn push_text(&mut self, text: &str) {
        let style = self.style();
        self.pieces.push(Piece::Text(text.to_string(), style));
    }

    pub(super) fn push_dim(&mut self, text: impl Into<String>) {
        self.pieces.push(Piece::Text(text.into(), Style::default().fg(DIM)));
    }

    pub(super) fn is_empty(&self) -> bool {
        self.pieces.is_empty()
    }

    pub(super) fn take(&mut self) -> Vec<Piece> {
        std::mem::take(&mut self.pieces)
    }
}

/// Mesma natureza de `piece` (texto ou código) com outro conteúdo; `Break` vira espaço.
pub(super) fn with_text(piece: &Piece, text: String) -> Piece {
    match piece {
        Piece::Code(_, st) => Piece::Code(text, *st),
        Piece::Text(_, st) => Piece::Text(text, *st),
        Piece::Break => Piece::Text(text, Style::default()),
    }
}

pub(super) fn clean_code(s: &str) -> String {
    s.trim_end_matches(['\n', '\r']).replace('\t', "    ")
}
