//! Pilha de listas e marcadores.

use super::*;

/// Marcador de item ainda não impresso e seu estilo.
pub(super) type Marker = (String, Style);

pub(super) struct ListLevel {
    pub(super) ordered: Option<u64>,
    /// Largura do marcador do item atual (usada para indentar o conteúdo).
    pub(super) width: usize,
    /// Marcador ainda não impresso (sai junto com a primeira linha do item).
    pub(super) pending: Option<Marker>,
}

/// Pilha de listas abertas (a mais interna é a última).
#[derive(Default)]
pub(super) struct ListStack {
    pub(super) levels: Vec<ListLevel>,
}

impl ListStack {
    pub(super) fn is_empty(&self) -> bool {
        self.levels.is_empty()
    }

    pub(super) fn open(&mut self, start: Option<u64>) {
        self.levels.push(ListLevel { ordered: start, width: 2, pending: None });
    }

    pub(super) fn close(&mut self) {
        self.levels.pop();
    }

    /// Começa um item na lista mais interna, com número ou bolinha conforme o nível.
    pub(super) fn start_item(&mut self) {
        let depth = self.levels.len();
        let Some(level) = self.levels.last_mut() else { return };
        let marker = match level.ordered {
            Some(n) => {
                level.ordered = Some(n.saturating_add(1));
                format!("{n}. ")
            }
            None => match depth {
                1 => "• ",
                2 => "◦ ",
                _ => "▪ ",
            }
            .to_string(),
        };
        level.set_marker(marker, BULLET);
    }

    pub(super) fn set_task_marker(&mut self, done: bool) {
        let Some(level) = self.levels.last_mut() else { return };
        let (marker, color) = if done { ("☑ ", BULLET) } else { ("☐ ", DIM) };
        level.set_marker(marker.to_string(), color);
    }

    /// Marca os marcadores de item como já impressos (saem só na primeira linha).
    /// Vale para todos os níveis: `- - x` abre dois itens antes de qualquer texto.
    pub(super) fn take_markers(&mut self) {
        for level in &mut self.levels {
            level.pending = None;
        }
    }

    /// Acrescenta a indentação (e os marcadores pendentes, só na primeira linha).
    pub(super) fn extend_prefixes(&self, first: &mut Vec<Span<'static>>, cont: &mut Vec<Span<'static>>) {
        for level in &self.levels {
            let pad = " ".repeat(level.width);
            match &level.pending {
                Some((marker, style)) => first.push(Span::styled(marker.clone(), *style)),
                None => first.push(Span::raw(pad.clone())),
            }
            cont.push(Span::raw(pad));
        }
    }
}

impl ListLevel {
    pub(super) fn set_marker(&mut self, marker: String, color: Color) {
        self.width = marker.width();
        self.pending = Some((marker, Style::default().fg(color)));
    }
}
