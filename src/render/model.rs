//! Tipos do resultado da renderização.

use std::sync::Arc;

use ratatui::text::Line;

use super::*;

/// Resultado da renderização.
#[derive(Default)]
pub struct Rendered {
    pub lines: Vec<Line<'static>>,
    /// Destino de cada link do documento, na ordem em que aparecem; o texto do link carrega o
    /// índice na cor do sublinhado (`link_id`). A TUI tira a marca antes de desenhar.
    pub links: Vec<String>,
    /// Índices (em `lines`) onde cada título começa.
    pub headings: Vec<usize>,
    /// Títulos com nível e texto, para o `--toc`.
    pub outline: Vec<OutlineEntry>,
        /// Tabelas desenhadas, em ordem, com o intervalo de linhas de cada uma (zoom `t`).
        pub tables: Vec<TableSpan>,
    /// Linha (0-based) do fonte onde cada linha renderizada começa. Vazio fora do Markdown.
    pub line_src: Vec<usize>,
    /// Blocos de código desenhados, em ordem.
    pub code_blocks: Vec<CodeSpan>,
    /// Itens de lista de tarefas, em ordem.
    pub tasks: Vec<TaskItem>,
    /// Maior quanto uma linha de código passa das colunas disponíveis (0 se nenhuma passa).
    /// Limite do deslocamento horizontal.
    pub code_overflow: usize,
    /// Imagens gráficas usadas, sem repetir `id`. Vazio sem `--graphics`.
    pub graphics: Vec<GraphicRef>,
    /// Diagramas Mermaid desenhados como imagem, em ordem (alvo do zoom de imagem).
    pub mermaids: Vec<MermaidSpan>,
}

/// Diagrama Mermaid desenhado como imagem: linhas renderizadas `[start, end)` e a chave do PNG
/// original em `ImageStore::mermaid_png`.
#[derive(Clone)]
pub struct MermaidSpan {
    pub start: usize,
    pub end: usize,
    pub key: String,
}

/// Bloco de código desenhado: linhas renderizadas `[start, end)` e o texto cru (já saneado).
#[derive(Clone)]
pub struct CodeSpan {
    pub start: usize,
    pub end: usize,
    #[cfg_attr(not(test), allow(dead_code))]
    pub lang: String,
    pub text: String,
}

/// Imagem que o terminal precisa receber (protocolo gráfico) para os placeholders aparecerem.
#[derive(Clone)]
pub struct GraphicRef {
    pub id: u32,
    pub cols: usize,
    pub rows: usize,
    pub png: Arc<Vec<u8>>,
}

/// Item de lista de tarefas e a linha renderizada onde ele começa.
#[derive(Clone, Copy)]
pub struct TaskItem {
    pub line: usize,
    pub done: bool,
}

#[allow(dead_code)] // `line` será usado pela navegação do sumário
pub struct OutlineEntry {
    pub line: usize,
    pub level: u8,
    pub title: String,
}

/// Tabela guardada para ser desenhada de novo em outra largura (zoom).
#[derive(Clone)]
pub struct TableData {
    pub(super) rows: Vec<Vec<Vec<Piece>>>,
    pub(super) aligns: Vec<Alignment>,
    pub(super) has_header: bool,
}

/// Onde uma tabela está em `Rendered.lines` (`start..end`) e como redesenhá-la.
pub struct TableSpan {
    pub start: usize,
    pub end: usize,
    pub data: TableData,
}

/// Tabela desenhada numa largura: as linhas, quantas ficam fixas no topo e quais colunas aparecem.
pub struct TableView {
    pub lines: Vec<Line<'static>>,
    /// Linhas fixas no topo (borda, cabeçalho e separador; só a borda sem cabeçalho).
    pub pinned: usize,
    /// Índice (0-based) da primeira coluna rolável mostrada, já limitado ao intervalo válido.
    pub first_col: usize,
    /// Colunas desenhadas, contando a primeira (fixa).
    pub shown_cols: usize,
    pub total_cols: usize,
}

impl TableData {
    pub fn total_cols(&self) -> usize {
        self.rows.iter().map(Vec::len).max().unwrap_or(0)
    }

    /// Desenha a tabela em `width` colunas, sem recuo. A coluna 0 fica sempre;
    /// as demais começam em `first_col`.
    pub fn draw(&self, width: usize, first_col: usize) -> TableView {
        layout_table(self, &[], &[], width, first_col)
    }
}
