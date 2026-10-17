use std::borrow::Cow;

const REPLACEMENT: char = '\u{FFFD}';

/// Ponto único de entrada do texto do documento: troca caracteres de controle
/// (ESC, BEL, C1, `\r` solto...) por U+FFFD, para que nada do Markdown vire
/// comando de terminal. Preserva `\n` e `\t`; `\r\n` vira `\n`.
pub fn sanitize(s: &str) -> Cow<'_, str> {
    let is_unsafe = |c: char| c.is_control() && c != '\n' && c != '\t';
    if !s.contains(is_unsafe) {
        return Cow::Borrowed(s);
    }
    let unified = s.replace("\r\n", "\n");
    Cow::Owned(
        unified
            .chars()
            .map(|c| if is_unsafe(c) { REPLACEMENT } else { c })
            .collect(),
    )
}



#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn texto_limpo_nao_aloca() {
        assert!(matches!(sanitize("olá\tmundo\n"), std::borrow::Cow::Borrowed(_)));
    }

    #[test]
    fn escape_e_controles_viram_substituto() {
        assert_eq!(sanitize("a\x1b[31mb\x07c\u{85}d"), "a\u{FFFD}[31mb\u{FFFD}c\u{FFFD}d");
    }

    #[test]
    fn crlf_vira_lf_e_cr_solto_vira_substituto() {
        assert_eq!(sanitize("a\r\nb\rc"), "a\nb\u{FFFD}c");
    }
}
