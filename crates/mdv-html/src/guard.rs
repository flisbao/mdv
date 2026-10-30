//! Tetos de tamanho, profundidade e memória da leitura de HTML.

/// Tetos da leitura. Todo laço que cresce com a entrada consulta um deles.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Limits {
    pub max_html_bytes: usize,
    pub max_parse_depth: usize,
    pub max_walk_depth: usize,
    pub max_elements: usize,
    pub max_css_bytes: usize,
    pub max_rules: usize,
    pub max_decls: usize,
    pub max_value_len: usize,
    pub max_lines: usize,
    pub max_table_cols: usize,
    pub max_grid_cols: usize,
    pub max_cell_chars: usize,
    pub max_pages: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            max_html_bytes: 4 * 1024 * 1024,
            max_parse_depth: 5000,
            max_walk_depth: 128,
            max_elements: 200_000,
            max_css_bytes: 512 * 1024,
            max_rules: 8000,
            max_decls: 200,
            max_value_len: 2048,
            max_lines: 200_000,
            max_table_cols: 100,
            max_grid_cols: 12,
            max_cell_chars: 4000,
            max_pages: 2000,
        }
    }
}

/// Elementos sem fechamento: não entram na pilha de profundidade.
const VOID: [&str; 14] = [
    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source",
    "track", "wbr",
];

/// Primeira ocorrência de `needle` em `hay` a partir de `from`.
fn find_bytes(hay: &[u8], from: usize, needle: &[u8]) -> Option<usize> {
    hay.get(from..)?
        .windows(needle.len())
        .position(|w| w == needle)
        .map(|p| p + from)
}

/// Índice logo depois do `>` que fecha a tag (ou fim do texto) e se a tag é `/>`.
/// Não respeita aspas de propósito: em caso de dúvida conta a mais, nunca a menos.
fn tag_end(b: &[u8], from: usize) -> (usize, bool) {
    match find_bytes(b, from, b">") {
        Some(p) => {
            let self_closing = p > from && b.get(p - 1) == Some(&b'/');
            (p + 1, self_closing)
        }
        None => (b.len(), false),
    }
}

/// Maior profundidade de tags abertas e não fechadas (varredura linear, sem recursão).
pub fn max_open_depth(html: &str) -> usize {
    let b = html.as_bytes();
    let lower = html.to_ascii_lowercase();
    let lb = lower.as_bytes();
    let (mut i, mut depth, mut max) = (0usize, 0usize, 0usize);
    while let Some(p) = find_bytes(b, i, b"<") {
        i = p + 1;
        if b.get(i..).is_some_and(|r| r.starts_with(b"!--")) {
            i = find_bytes(b, i, b"-->").map_or(b.len(), |e| e + 3);
            continue;
        }
        if matches!(b.get(i), Some(b'!') | Some(b'?')) {
            i = tag_end(b, i).0;
            continue;
        }
        let closing = b.get(i) == Some(&b'/');
        if closing {
            i += 1;
        }
        let start = i;
        while b.get(i).is_some_and(|c| c.is_ascii_alphanumeric()) {
            i += 1;
        }
        let name = lower.get(start..i).unwrap_or("");
        if name.is_empty() {
            continue;
        }
        let (end, self_closing) = tag_end(b, i);
        i = end;
        if closing {
            depth = depth.saturating_sub(1);
            continue;
        }
        if self_closing || VOID.contains(&name) {
            continue;
        }
        // Conteúdo de script e style é texto: pula até o fechamento (inclusive a tag de fechamento).
        let close: Option<&[u8]> = match name {
            "script" => Some(b"</script"),
            "style" => Some(b"</style"),
            _ => None,
        };
        if let Some(close) = close {
            i = find_bytes(lb, i, close).map_or(b.len(), |c| tag_end(b, c).0);
            continue;
        }
        depth += 1;
        max = max.max(depth);
    }
    max
}

/// Recusa HTML grande demais ou aninhado demais antes de qualquer parse.
pub fn check_html(html: &str, lim: &Limits) -> Result<(), String> {
    if html.len() > lim.max_html_bytes {
        return Err("HTML grande demais".to_string());
    }
    if max_open_depth(html) > lim.max_parse_depth {
        return Err("HTML aninhado demais".to_string());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profundidade_simples() {
        assert_eq!(max_open_depth("<div><p>a</p></div>"), 2);
    }

    #[test]
    fn ignora_void_e_comentario() {
        assert_eq!(max_open_depth("<div><br><img src=x><!-- <b><b> --></div>"), 1);
    }

    #[test]
    fn ignora_script_e_style() {
        assert_eq!(max_open_depth("<div><script>if (a<b) {'<div><div>'}</script></div>"), 1);
    }

    #[test]
    fn fechamento_sem_abertura_nao_underflow() {
        assert_eq!(max_open_depth("</div></div><p>x"), 1);
    }

    #[test]
    fn html_aninhado_demais_e_recusado() {
        let html = "<div>".repeat(6000);
        let err = check_html(&html, &Limits::default()).unwrap_err();
        assert!(err.contains("aninhado"), "{err}");
    }

    #[test]
    fn html_grande_demais() {
        let lim = Limits {
            max_html_bytes: 10,
            ..Default::default()
        };
        let err = check_html(&"x".repeat(11), &lim).unwrap_err();
        assert!(err.contains("grande demais"), "{err}");
    }
}
