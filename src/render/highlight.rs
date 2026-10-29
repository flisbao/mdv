//! Blocos de código: realce (syntect), diff e desenho do bloco.

use super::*;

/// Orçamento de realce por documento: o realce custa ~3 s/MiB e roda a cada re-render,
/// então blocos que estourem o que resta do orçamento (ou o teto de um único bloco) saem sem cor.
pub(crate) const MAX_HIGHLIGHT_BYTES: usize = 256 * 1024;

/// Blocos ```diff e ```patch recebem cores de diff em vez do realce de sintaxe.
pub(super) fn is_diff_lang(lang: &str) -> bool {
    lang.eq_ignore_ascii_case("diff") || lang.eq_ignore_ascii_case("patch")
}

/// Estilo de uma linha de diff unificado, pelo primeiro caractere.
pub(super) fn diff_style(line: &str) -> Style {
    let base = Style::default().bg(CODE_BG);
    if line.starts_with("+++") || line.starts_with("---") || line.starts_with("diff ") || line.starts_with("index ") {
        base.fg(H4).add_modifier(Modifier::BOLD)
    } else if line.starts_with("@@") {
        base.fg(DIFF_HUNK_FG)
    } else if line.starts_with('+') {
        Style::default().fg(DIFF_ADD_FG).bg(DIFF_ADD_BG)
    } else if line.starts_with('-') {
        Style::default().fg(DIFF_DEL_FG).bg(DIFF_DEL_BG)
    } else {
        base.fg(QUOTE)
    }
}

pub(super) fn hl() -> &'static (SyntaxSet, Theme) {
    static HL: OnceLock<(SyntaxSet, Theme)> = OnceLock::new();
    HL.get_or_init(|| {
        let ss = SyntaxSet::load_defaults_newlines();
        let mut ts = ThemeSet::load_defaults();
        let theme = ts
            .themes
            .remove("base16-ocean.dark")
            .expect("tema embutido `base16-ocean.dark` ausente no syntect");
        (ss, theme)
    })
}

/// Bloco de código cercado ou indentado, acumulado até o fim do bloco.
pub(super) struct CodeBlock {
    pub(super) lang: String,
    pub(super) buf: String,
}

impl CodeBlock {
    pub(super) fn new(kind: CodeBlockKind<'_>) -> Self {
        let lang = match kind {
            CodeBlockKind::Fenced(info) => info.split_whitespace().next().unwrap_or("").to_string(),
            CodeBlockKind::Indented => String::new(),
        };
        CodeBlock { lang, buf: String::new() }
    }
}

impl Renderer<'_> {
    pub(super) fn render_code(&mut self, block: &CodeBlock) {
        let Some(label) = self.mermaid_figure(block) else { return };
        let (ss, theme) = hl();
        let syntax = if block.lang.is_empty() || block.buf.len() > self.highlight_left {
            ss.find_syntax_plain_text()
        } else {
            self.highlight_left -= block.buf.len();
            ss.find_syntax_by_token(&block.lang)
                .unwrap_or_else(|| ss.find_syntax_plain_text())
        };
        let mut h = HighlightLines::new(syntax, theme);
        let is_diff = is_diff_lang(&block.lang);
        let (first, cont) = self.prefixes();
        self.take_marker();
        let avail = self.width.saturating_sub(spans_width(&cont) + 2).max(MIN_BODY_WIDTH);
        let dim = Style::default().fg(DIM);

        let start = self.out.lines.len();
        let header = if label.is_empty() { "╭─".to_string() } else { format!("╭─ {label}") };
        let mut v = first;
        v.push(Span::styled(header, dim));
        self.out.lines.push(Line::from(v));

        for line in LinesWithEndings::from(&block.buf) {
            if mem::exceeded() {
                break;
            }
            let pieces: Vec<Styled> = if is_diff {
                vec![(clean_code(line), diff_style(line))]
            } else {
                match h.highlight_line(line, ss) {
                    Ok(regions) => regions
                        .iter()
                        .map(|(st, txt)| {
                            let fg = Color::Rgb(st.foreground.r, st.foreground.g, st.foreground.b);
                            (clean_code(txt), Style::default().fg(fg).bg(CODE_BG))
                        })
                        .filter(|(t, _)| !t.is_empty())
                        .collect(),
                    Err(_) => vec![(clean_code(line), Style::default().bg(CODE_BG))],
                }
            };
            let line_width: usize = pieces.iter().map(|(t, _)| t.width()).sum();
            self.out.code_overflow = self.out.code_overflow.max(line_width.saturating_sub(avail));
            let chunks = match self.code_scroll {
                Some(skip) => vec![window_by_width(&pieces, skip, avail)],
                None => split_by_width(&pieces, avail),
            };
            for chunk in chunks {
                let w: usize = chunk.iter().map(|(t, _)| t.width()).sum();
                let mut spans = cont.clone();
                spans.push(Span::styled("│ ", dim));
                for (t, st) in chunk {
                    spans.push(Span::styled(t, st));
                }
                spans.push(Span::styled(
                    " ".repeat(avail.saturating_sub(w)),
                    Style::default().bg(CODE_BG),
                ));
                self.out.lines.push(Line::from(spans));
            }
        }

        let mut v = cont;
        v.push(Span::styled("╰─", dim));
        self.out.lines.push(Line::from(v));
        self.out.code_blocks.push(CodeSpan {
            start,
            end: self.out.lines.len(),
            lang: block.lang.clone(),
            text: block.buf.clone(),
        });
    }
}
