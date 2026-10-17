//! Busca incremental (`/`, `n`/`N`) sobre o texto das linhas já renderizadas.

use super::*;

/// Busca incremental: texto em edição, termo confirmado e linhas encontradas.
#[derive(Default)]
pub(crate) struct SearchState {
    /// Texto sendo digitado (`Some` enquanto o prompt `/` está aberto).
    pub(crate) input: Option<String>,
    pub(crate) query: String,
    pub(crate) matches: Vec<usize>,
    pub(crate) current: usize,
}

impl SearchState {
    pub(crate) fn refresh(&mut self, lines: &[Line<'_>]) {
        self.matches.clear();
        if self.query.is_empty() {
            return;
        }
        let q = fold(&self.query);
        for (i, l) in lines.iter().enumerate() {
            if find_from(&fold(&line_text(l)), &q, 0).is_some() {
                self.matches.push(i);
            }
        }
        if self.current >= self.matches.len() {
            self.current = 0;
        }
    }

    /// Confirma `query` e escolhe a primeira ocorrência a partir de `scroll`.
    pub(crate) fn commit(&mut self, query: String, lines: &[Line<'_>], scroll: usize) {
        self.query = query;
        self.refresh(lines);
        self.current = self.matches.iter().position(|&m| m >= scroll).unwrap_or(0);
    }

    pub(crate) fn clear(&mut self) {
        self.query.clear();
        self.matches.clear();
    }

    pub(crate) fn step(&mut self, forward: bool) {
        let n = self.matches.len();
        if n == 0 {
            return;
        }
        self.current = if forward {
            (self.current + 1) % n
        } else {
            (self.current + n - 1) % n
        };
    }

    pub(crate) fn current_line(&self) -> Option<usize> {
        self.matches.get(self.current).copied()
    }
}

pub(crate) fn line_text(l: &Line<'_>) -> String {
    l.spans.iter().map(|s| s.content.as_ref()).collect()
}

/// Minúscula caractere a caractere (um `char` vira um `char`): índices de `Vec<char>` continuam
/// alinhados com a linha original mesmo onde `to_lowercase` mudaria o tamanho (ex.: `İ`).
pub(crate) fn fold_char(c: char) -> char {
    let mut it = c.to_lowercase();
    match (it.next(), it.next()) {
        (Some(l), None) => l,
        _ => c,
    }
}

pub(crate) fn fold(s: &str) -> Vec<char> {
    s.chars().map(fold_char).collect()
}

/// Primeira posição de `needle` em `hay` a partir de `from`. `needle` vazio nunca casa.
pub(crate) fn find_from(hay: &[char], needle: &[char], from: usize) -> Option<usize> {
    if needle.is_empty() || hay.len() < needle.len() {
        return None;
    }
    (from..=hay.len() - needle.len()).find(|&i| hay[i..i + needle.len()] == *needle)
}

/// Destaca as ocorrências da busca `q` (sem diferenciar caixa) na linha, inclusive as que
/// atravessam trechos de estilos diferentes: cada caractere guarda o estilo do seu trecho.
pub(crate) fn highlight_line(line: &Line<'static>, q: &str) -> Line<'static> {
    let needle = fold(q);
    if needle.is_empty() {
        return line.clone();
    }
    let cells: Vec<(char, Style)> = line
        .spans
        .iter()
        .flat_map(|s| s.content.chars().map(move |c| (c, s.style)))
        .collect();
    let hay: Vec<char> = cells.iter().map(|&(c, _)| fold_char(c)).collect();
    let mut marked = vec![false; cells.len()];
    let mut start = 0;
    let mut any = false;
    while let Some(i) = find_from(&hay, &needle, start) {
        marked[i..i + needle.len()].fill(true);
        start = i + needle.len();
        any = true;
    }
    if !any {
        return line.clone();
    }
    let hl = Style::default()
        .bg(Color::Rgb(255, 213, 79))
        .fg(Color::Black);
    let mut out: Vec<Span<'static>> = Vec::new();
    let mut buf = String::new();
    let mut cur: Option<Style> = None;
    for (&(c, st), &m) in cells.iter().zip(&marked) {
        let style = if m { st.patch(hl) } else { st };
        if cur != Some(style) {
            if let Some(prev) = cur {
                out.push(Span::styled(std::mem::take(&mut buf), prev));
            }
            cur = Some(style);
        }
        buf.push(c);
    }
    if let Some(prev) = cur {
        out.push(Span::styled(buf, prev));
    }
    Line::from(out)
}

impl App {
    pub(crate) fn set_search(&mut self, query: String) {
        self.search
            .commit(query, &self.doc.rendered.lines, self.view.scroll);
        self.goto_match();
    }

    pub(crate) fn goto_match(&mut self) {
        if let Some(line) = self.search.current_line() {
            self.scroll_to(line.saturating_sub(self.view.height / 3));
        }
    }

    pub(crate) fn step_match(&mut self, forward: bool) {
        self.search.step(forward);
        self.goto_match();
    }

    /// Trata uma tecla com o prompt de busca aberto (`buf` é o texto digitado até agora).
    pub(crate) fn on_search_key(&mut self, code: KeyCode, mut buf: String) {
        match code {
            KeyCode::Esc => {}
            KeyCode::Enter => self.set_search(buf),
            KeyCode::Backspace => {
                buf.pop();
                self.search.input = Some(buf);
            }
            KeyCode::Char(c) => {
                buf.push(c);
                self.search.input = Some(buf);
            }
            _ => self.search.input = Some(buf),
        }
    }
}
