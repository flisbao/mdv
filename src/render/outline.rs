//! Seções, tarefas e índice (`--toc`) sobre um `Rendered`.

use super::*;

/// Linhas renderizadas `[início, fim)` da seção `idx`: do título até o próximo de nível
/// igual ou menor, ou até `total`.
pub fn section_range(outline: &[OutlineEntry], idx: usize, total: usize) -> (usize, usize) {
    let e = &outline[idx];
    let end = outline[idx + 1..].iter().find(|n| n.level <= e.level).map_or(total, |n| n.line);
    (e.line, end)
}

/// Tarefas feitas e total num trecho do documento.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct TaskCount {
    pub done: usize,
    pub total: usize,
}

/// Conta as tarefas cujas linhas estão em `[from, to)`.
pub fn count_tasks(tasks: &[TaskItem], from: usize, to: usize) -> TaskCount {
    let mut c = TaskCount::default();
    for t in tasks.iter().filter(|t| t.line >= from && t.line < to) {
        c.total += 1;
        if t.done {
            c.done += 1;
        }
    }
    c
}

/// Tarefas da seção `idx`, contando as subseções.
pub fn section_tasks(r: &Rendered, idx: usize) -> TaskCount {
    let (start, end) = section_range(&r.outline, idx, r.lines.len());
    count_tasks(&r.tasks, start, end)
}

/// Título mais profundo que começa em `line` ou antes (a seção que contém a linha).
pub fn section_at(outline: &[OutlineEntry], line: usize) -> Option<usize> {
    outline.iter().rposition(|e| e.line <= line)
}

/// Markdown cru da seção `idx` (com o título), fatiado do fonte pelo mapa `line_src`.
pub fn section_source(md: &str, r: &Rendered, idx: usize) -> Option<String> {
    if idx >= r.outline.len() || r.line_src.len() != r.lines.len() {
        return None;
    }
    let (start, end) = section_range(&r.outline, idx, r.lines.len());
    let first = *r.line_src.get(start)?;
    let last = r.line_src.get(end).copied().unwrap_or_else(|| md.lines().count());
    let body: Vec<&str> = md.lines().skip(first).take(last.saturating_sub(first)).collect();
    let mut text = body.join("\n").trim_end().to_string();
    text.push('\n');
    Some(text)
}

/// Índice dos títulos, um por linha, indentado conforme o nível.
pub fn toc(r: &Rendered) -> String {
    let mut out = String::new();
    for (i, e) in r.outline.iter().enumerate() {
        let _ = write!(out, "{}{}", "  ".repeat(usize::from(e.level.saturating_sub(1))), e.title);
        let c = section_tasks(r, i);
        if c.total > 0 {
            let _ = write!(out, " [{}/{}]", c.done, c.total);
        }
        out.push('\n');
    }
    out
}
