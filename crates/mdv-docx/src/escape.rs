//! Escape de texto do documento para Markdown. Cada função é o ponto único de escape do trecho
//! emitido: nunca escape duas vezes o mesmo texto.

/// Escapa os caracteres com significado inline (`\ * _ ` [ ] < ~`).
pub(crate) fn inline(s: &str) -> String {
    let mut saida = String::with_capacity(s.len());
    for c in s.chars() {
        if matches!(c, '\\' | '*' | '_' | '`' | '[' | ']' | '<' | '~') {
            saida.push('\\');
        }
        saida.push(c);
    }
    saida
}

/// Escapa texto já inline-escapado para o começo de linha (`# > - + = |` e `N.`/`N)`).
///
/// Roda sobre texto já escapado e só no começo da linha: `#`, `>`, `-`, `+` e `=` seriam
/// marcadores de bloco, `|` poderia forjar uma tabela GFM, e `N.`/`N)` viraria lista numerada. Depois do primeiro caractere
/// o texto já está seguro e não precisa de nova barra.
pub(crate) fn inicio_de_linha(s: &str) -> String {
    if s.starts_with(['#', '>', '-', '+', '=', '|']) {
        return format!("\\{s}");
    }
    let digitos = s.bytes().take_while(u8::is_ascii_digit).count();
    if (1..=9).contains(&digitos) && s[digitos..].starts_with(['.', ')']) {
        return format!("{}\\{}", &s[..digitos], &s[digitos..]);
    }
    s.to_string()
}

/// Escapa texto de célula de tabela: inline, mais `|`; quebras e tabs viram espaço.
pub(crate) fn celula(s: &str) -> String {
    let plano: String = s
        .chars()
        .map(|c| if matches!(c, '\n' | '\r' | '\t') { ' ' } else { c })
        .collect();
    inline(&plano).replace('|', "\\|")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapa_inline_basico() {
        assert_eq!(inline("a*b_c"), "a\\*b\\_c");
    }

    #[test]
    fn barra_invertida_dobra() {
        assert_eq!(inline("a\\b"), "a\\\\b");
    }

    #[test]
    fn escapa_tag_html() {
        assert_eq!(inline("<tag>"), "\\<tag>");
    }

    #[test]
    fn escapa_colchetes() {
        assert_eq!(inline("[x]"), "\\[x\\]");
    }

    #[test]
    fn escapa_til() {
        assert_eq!(inline("a~b"), "a\\~b");
    }

    #[test]
    fn escapa_crase() {
        assert_eq!(inline("a`b"), "a\\`b");
    }

    #[test]
    fn acentos_e_e_comercial_intactos() {
        assert_eq!(inline("café & chá"), "café & chá");
    }

    #[test]
    fn inicio_de_linha_marca_titulo() {
        assert_eq!(inicio_de_linha("# t"), "\\# t");
    }

    #[test]
    fn inicio_de_linha_marca_citacao() {
        assert_eq!(inicio_de_linha("> q"), "\\> q");
    }

    #[test]
    fn inicio_de_linha_marca_hifen_e_mais() {
        assert_eq!(inicio_de_linha("- a"), "\\- a");
        assert_eq!(inicio_de_linha("+ a"), "\\+ a");
    }

    #[test]
    fn inicio_de_linha_marca_igual() {
        assert_eq!(inicio_de_linha("=== "), "\\=== ");
    }

    #[test]
    fn inicio_de_linha_marca_lista_numerada() {
        assert_eq!(inicio_de_linha("1. a"), "1\\. a");
        assert_eq!(inicio_de_linha("1) a"), "1\\) a");
    }

    #[test]
    fn inicio_de_linha_ano_intacto() {
        assert_eq!(inicio_de_linha("2024 foi"), "2024 foi");
    }

    #[test]
    fn inicio_de_linha_texto_intacto() {
        assert_eq!(inicio_de_linha("texto"), "texto");
    }

    #[test]
    fn inicio_de_linha_marca_barra_vertical() {
        assert_eq!(inicio_de_linha("|a|"), "\\|a|");
    }

    #[test]
    fn inicio_de_linha_regra_horizontal() {
        assert_eq!(inicio_de_linha("---"), "\\---");
    }

    #[test]
    fn celula_escapa_pipe_e_quebra() {
        assert_eq!(celula("a|b\nc"), "a\\|b c");
    }
}
