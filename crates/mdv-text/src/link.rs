//! Regras de links: âncoras, `%XX`, esquemas externos e confinamento de caminhos.

use std::path::Path;

/// Âncora de um título, como no GitHub: minúsculas, espaço vira `-`, pontuação some.
pub fn slugify(title: &str) -> String {
    title
        .trim()
        .to_lowercase()
        .chars()
        .filter_map(|c| {
            if c.is_alphanumeric() || c == '_' || c == '-' {
                Some(c)
            } else if c.is_whitespace() {
                Some('-')
            } else {
                None
            }
        })
        .collect()
}


/// Decodifica `%XX` (bytes inválidos viram U+FFFD).
pub fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            let hex = std::str::from_utf8(&b[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok());
            if let Some(v) = hex {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Esquema (`https:`, `mailto:`, `data:`...) ou `//host`: não é conferido (nada de rede).
pub fn is_external(dest: &str) -> bool {
    if dest.starts_with("//") {
        return true;
    }
    match dest.split_once(':') {
        Some((scheme, _)) => {
            !scheme.is_empty()
                && scheme.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
                && scheme.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '.' | '-'))
                && scheme.len() > 1 // `C:\x` (Windows) não é um esquema
        }
        None => false,
    }
}

/// O caminho (já decodificado) é relativo, sem unidade/UNC e sem `..`? Só esses são consultados no
/// disco: um caminho absoluto substituiria a base no `join` (oráculo de existência de qualquer
/// arquivo), um `\\\\servidor\\pasta` abriria conexão de rede no Windows (vazaria credenciais) e
/// um `..` sairia da pasta do documento. Links simbólicos dentro da pasta continuam sendo seguidos.
pub fn is_confined(p: &str) -> bool {
    use std::path::Component;
    if p.starts_with(['/', '\\']) || p.contains(':') {
        return false;
    }
    Path::new(p)
        .components()
        .all(|c| !matches!(c, Component::Prefix(_) | Component::RootDir | Component::ParentDir))
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slug_segue_a_regra_do_github() {
        assert_eq!(slugify("Instalação Rápida"), "instalação-rápida");
        assert_eq!(slugify("O que é isso?"), "o-que-é-isso");
        assert_eq!(slugify("a_b - c"), "a_b---c");
        assert_eq!(slugify("  Olá, mundo!  "), "olá-mundo");
    }

    #[test]
    fn percentual_e_decodificado() {
        assert_eq!(percent_decode("a%20b.md"), "a b.md");
        assert_eq!(percent_decode("%2Fetc"), "/etc");
    }

    #[test]
    fn percentual_perto_de_caractere_multibyte_nao_quebra() {
        assert_eq!(percent_decode("x%é.md"), "x%é.md");
        assert_eq!(percent_decode("100%"), "100%");
    }

    #[test]
    fn caminho_com_pai_nao_e_confinado() {
        assert!(is_confined("a.md"));
        assert!(is_confined("./a.md"));
        assert!(is_confined("sub/a.md"));
        assert!(!is_confined("../a.md"));
        assert!(!is_confined("sub/../a.md"));
        assert!(!is_confined("a/../../b.md"));
        assert!(!is_confined(".."));
    }

    #[test]
    fn absoluto_unc_ou_unidade_nunca_e_confinado() {
        for p in ["/etc/passwd", "\\\\host\\share\\x.md", "C:\\x.md", "C:x.md"] {
            assert!(!is_confined(p), "{p}");
        }
    }

    #[test]
    fn esquema_e_externo_mas_unidade_do_windows_nao() {
        assert!(is_external("https://x.y"));
        assert!(is_external("mailto:a@b"));
        assert!(is_external("//host/x"));
        assert!(!is_external("C:\\x.md"));
        assert!(!is_external("a/b.md"));
    }
}
