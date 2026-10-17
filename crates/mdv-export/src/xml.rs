//! Escape de texto para XML 1.0 (DOCX e ODT).

/// Caracteres que XML 1.0 não admite (controles, exceto tab e quebra de linha) viram `?`.
fn is_valid_xml_char(c: char) -> bool {
    matches!(c, '\t' | '\n' | '\r') || (c >= ' ' && c != '\u{FFFE}' && c != '\u{FFFF}')
}

/// Escapa `&`, `<`, `>`, `"` e `'` e troca caracteres proibidos por `?`.
pub fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            c if is_valid_xml_char(c) => out.push(c),
            _ => out.push('?'),
        }
    }
    out
}

/// Só para testes: confere que as marcas XML abrem e fecham em ordem (sem validar o esquema).
#[cfg(test)]
pub fn assert_balanced(xml: &str) {
    let mut stack: Vec<String> = Vec::new();
    let mut rest = xml;
    while let Some(start) = rest.find('<') {
        let end = rest[start..].find('>').map(|e| start + e).unwrap_or_else(|| panic!("'<' sem '>' perto de: {}", &rest[start..start.saturating_add(60).min(rest.len())]));
        let tag = &rest[start + 1..end];
        rest = &rest[end + 1..];
        if tag.starts_with('?') || tag.starts_with('!') || tag.ends_with('/') {
            continue;
        }
        let name = |t: &str| t.split_whitespace().next().unwrap_or("").to_string();
        if let Some(close) = tag.strip_prefix('/') {
            assert_eq!(stack.pop().as_deref(), Some(name(close).as_str()), "fechamento fora de ordem: </{close}>");
        } else {
            stack.push(name(tag));
        }
    }
    assert!(stack.is_empty(), "marcas sem fechar: {stack:?}");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapa_os_cinco_caracteres_especiais() {
        assert_eq!(escape("a<b>&\"c'd"), "a&lt;b&gt;&amp;&quot;c&apos;d");
    }

    #[test]
    fn controles_viram_interrogacao_e_tab_fica() {
        assert_eq!(escape("a\u{0}b\u{8}c\td\u{FFFF}"), "a?b?c\td?");
    }

    #[test]
    fn assert_balanced_aceita_bem_formado_e_recusa_o_resto() {
        assert_balanced("<?xml version=\"1.0\"?><a x=\"1\"><b/><c>t</c></a>");
        assert!(std::panic::catch_unwind(|| assert_balanced("<a><b></a></b>")).is_err());
        assert!(std::panic::catch_unwind(|| assert_balanced("<a>")).is_err());
    }

    #[test]
    fn acentos_e_emoji_passam() {
        assert_eq!(escape("ação 😀"), "ação 😀");
    }
}
