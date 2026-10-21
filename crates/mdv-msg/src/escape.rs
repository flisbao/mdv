//! Escape de texto vindo do e-mail para Markdown. Cada função é o ponto único de escape
//! do trecho emitido: nunca escape duas vezes o mesmo texto.

use crate::MAX_CELL_CHARS;

/// Escapa os caracteres com significado inline (`\ * _ ` [ ] < ~`).
pub(crate) fn inline(s: &str) -> String {
    escapa(s, false)
}

/// Escapa para o começo de linha: além do inline, `#`, `>`, `-`, `+`, `=` e `N.`/`N)`.
pub(crate) fn line_start(s: &str) -> String {
    let esc = inline(s);
    if esc.starts_with(['#', '>', '-', '+', '=']) {
        return format!("\\{esc}");
    }
    let digitos = esc.chars().take_while(char::is_ascii_digit).count();
    if (1..=9).contains(&digitos) && matches!(esc[digitos..].chars().next(), Some('.' | ')')) {
        let (a, b) = esc.split_at(digitos);
        return format!("{a}\\{b}");
    }
    esc
}

/// Escapa texto de célula de tabela: inline, mais `|`; quebras viram espaço.
/// O texto bruto é cortado em `MAX_CELL_CHARS` caracteres, com reticências.
pub(crate) fn cell(s: &str) -> String {
    let cortado = corta(s, false);
    let plano: String = cortado
        .chars()
        .map(|c| if matches!(c, '\n' | '\r' | '\t') { ' ' } else { c })
        .collect();
    escapa(&plano, true)
}

/// Corta um trecho já em Markdown (célula inteira) em `MAX_CELL_CHARS` caracteres,
/// sem deixar uma barra de escape solta antes das reticências.
pub(crate) fn cap(md: &str) -> String {
    corta(md, true)
}

fn escapa(s: &str, pipe: bool) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if matches!(c, '\\' | '*' | '_' | '`' | '[' | ']' | '<' | '~') || (pipe && c == '|') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

fn corta(s: &str, markdown: bool) -> String {
    if s.chars().count() <= MAX_CELL_CHARS {
        return s.to_string();
    }
    let mut t: String = s.chars().take(MAX_CELL_CHARS - 1).collect();
    if markdown && t.chars().rev().take_while(|&c| c == '\\').count() % 2 == 1 {
        t.pop();
    }
    t.push('…');
    t
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inicio_de_linha_escapa_marcas_de_bloco() {
        assert_eq!(line_start("# x"), "\\# x");
        assert_eq!(line_start("- x"), "\\- x");
        assert_eq!(line_start("12. x"), "12\\. x");
        assert_eq!(line_start("3) x"), "3\\) x");
        assert_eq!(line_start("12 x"), "12 x");
    }

    #[test]
    fn celula_escapa_barra_vertical_e_quebras() {
        assert_eq!(cell("a|b\nc"), "a\\|b c");
    }

    #[test]
    fn celula_corta_no_teto_com_reticencias() {
        let longo = "x".repeat(MAX_CELL_CHARS + 10);
        let r = cell(&longo);
        assert_eq!(r.chars().count(), MAX_CELL_CHARS);
        assert!(r.ends_with('…'));
    }

    #[test]
    fn cap_nao_deixa_barra_solta() {
        let md = format!("{}\\*y", "x".repeat(MAX_CELL_CHARS - 2));
        let r = cap(&md);
        assert_eq!(r, format!("{}…", "x".repeat(MAX_CELL_CHARS - 2)));
    }
}
