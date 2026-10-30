//! Destino de link Markdown: filtro de esquemas e codificação da URL.

/// Diz se o esquema da URL é um dos que viram link (`http`, `https`, `mailto`).
///
/// Só estes três esquemas viram link clicável; `javascript:`, `file:` e `data:` ficam como texto,
/// porque abrir esses alvos a partir de um documento baixado seria um risco.
pub(crate) fn permitido(url: &str) -> bool {
    match url.split_once(':') {
        Some((esquema, _)) if !esquema.is_empty() => ["http", "https", "mailto"]
            .iter()
            .any(|valido| esquema.eq_ignore_ascii_case(valido)),
        _ => false,
    }
}

/// Codifica a URL para caber em `[texto](destino)`.
///
/// Espaço e parênteses fecham cedo a sintaxe `[texto](destino)`, `<`/`>` ficam ambíguos no
/// Markdown e a barra invertida escaparia o `)` final; por isso são codificados. Controles são descartados, não codificados.
pub(crate) fn destino(url: &str) -> String {
    let mut saida = String::with_capacity(url.len());
    for c in url.chars() {
        match c {
            _ if c.is_control() => {}
            ' ' => saida.push_str("%20"),
            '(' => saida.push_str("%28"),
            ')' => saida.push_str("%29"),
            '<' => saida.push_str("%3C"),
            '>' => saida.push_str("%3E"),
            '\\' => saida.push_str("%5C"),
            _ => saida.push(c),
        }
    }
    saida
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permite_http_https_e_mailto() {
        assert!(permitido("https://a.org"));
        assert!(permitido("HTTP://A.ORG"));
        assert!(permitido("mailto:x@y.z"));
    }

    #[test]
    fn recusa_outros_esquemas() {
        assert!(!permitido("javascript:alert(1)"));
        assert!(!permitido("file:///etc/passwd"));
        assert!(!permitido("data:text/html,x"));
        assert!(!permitido("vbscript:x"));
    }

    #[test]
    fn recusa_sem_esquema() {
        assert!(!permitido(""));
        assert!(!permitido("www.exemplo.com"));
        assert!(!permitido("../x.md"));
    }

    #[test]
    fn codifica_espaco_e_parenteses() {
        assert_eq!(destino("https://a.org/x y(1)"), "https://a.org/x%20y%281%29");
    }

    #[test]
    fn codifica_menor_e_maior() {
        let d = destino("https://a.org/<b>");
        assert!(!d.contains('<') && !d.contains('>'));
        assert_eq!(d, "https://a.org/%3Cb%3E");
    }

    #[test]
    fn codifica_barra_invertida() {
        assert_eq!(destino("https://a.org/x\\"), "https://a.org/x%5C");
    }

    #[test]
    fn descarta_controle() {
        assert_eq!(destino("https://a.org/\x07x\ty"), "https://a.org/xy");
    }
}
