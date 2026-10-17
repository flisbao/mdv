//! Janela de rolagem sobre as linhas renderizadas.

use super::*;

/// Janela de rolagem sobre as linhas renderizadas.
#[derive(Default)]
pub(crate) struct Viewport {
    pub(crate) scroll: usize,
    pub(crate) height: usize,
}

impl Viewport {
    pub(crate) fn max_scroll(&self, total: usize) -> usize {
        total.saturating_sub(self.height)
    }

    pub(crate) fn scroll_by(&mut self, delta: isize, total: usize) {
        let target = if delta.is_negative() {
            self.scroll.saturating_sub(delta.unsigned_abs())
        } else {
            self.scroll.saturating_add(delta.unsigned_abs())
        };
        self.scroll_to(target, total);
    }

    pub(crate) fn scroll_to(&mut self, line: usize, total: usize) {
        self.scroll = line.min(self.max_scroll(total));
    }
}

impl App {
    pub(crate) fn total_lines(&self) -> usize {
        self.doc.rendered.lines.len()
    }

    pub(crate) fn scroll_by(&mut self, delta: isize) {
        self.view.scroll_by(delta, self.total_lines());
    }

    pub(crate) fn scroll_to(&mut self, line: usize) {
        self.view.scroll_to(line, self.total_lines());
    }

    /// Linhas renderizadas que cabem na janela.
    pub(crate) fn window(&self) -> std::ops::Range<usize> {
        let total = self.total_lines();
        self.view.scroll.min(total)..(self.view.scroll + self.view.height).min(total)
    }
}
