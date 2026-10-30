//! Escape de texto do documento para Markdown. Cada função é o ponto único de escape do trecho
//! emitido: nunca escape duas vezes o mesmo texto.

/// Escapa os caracteres com significado inline (`\ * _ ` [ ] < ~`) e o `&` que abriria uma
/// entidade (`&amp;`, `&#65;`), que o leitor Markdown mostraria como outro caractere.
pub(crate) fn inline(s: &str) -> String {
    let mut saida = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        let entidade = c == '&' && chars.peek().is_some_and(|p| p.is_ascii_alphanumeric() || *p == '#');
        if entidade || matches!(c, '\\' | '*' | '_' | '`' | '[' | ']' | '<' | '~') {
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
    if s.starts_with(['#', '>', '-', '+', '=', '|', ':']) {
        return format!("\\{s}");
    }
    let digitos = s.bytes().take_while(u8::is_ascii_digit).count();
    if (1..=9).contains(&digitos) && s[digitos..].starts_with(['.', ')']) {
        return format!("{}\\{}", &s[..digitos], &s[digitos..]);
    }
    s.to_string()
}

/// Maior sequência seguida de crases do texto; a cerca de código precisa ser mais longa que ela.
pub(crate) fn maior_crases(s: &str) -> usize {
    let mut maior = 0;
    let mut atual = 0;
    for c in s.chars() {
        atual = if c == '`' { atual + 1 } else { 0 };
        maior = maior.max(atual);
    }
    maior
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
    fn maior_sequencia_de_crases() {
        assert_eq!(maior_crases("a`b``c```d`"), 3);
        assert_eq!(maior_crases("sem"), 0);
    }

    #[test]
    fn e_comercial_de_entidade_e_escapado() {
        assert_eq!(inline("a &amp; b"), "a \\&amp; b");
        assert_eq!(inline("&#65;"), "\\&#65;");
        assert_eq!(inline("R&D"), "R\\&D");
    }

    #[test]
    fn dois_pontos_no_comeco_da_linha_sao_escapados() {
        assert_eq!(inicio_de_linha(":--"), "\\:--");
        assert_eq!(inicio_de_linha("a:b"), "a:b");
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
