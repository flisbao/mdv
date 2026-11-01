//! Escape de texto de célula e de título para Markdown. Quebras e tabulações viram espaço,
//! para que nenhum texto de célula quebre a linha da tabela.

/// Escapa a sintaxe Markdown inline e troca quebras de linha e tabulações por um espaço.
pub(crate) fn escapar(texto: &str) -> String {
    let mut saida = String::with_capacity(texto.len());
    for c in texto.chars() {
        match c {
            '\r' | '\n' | '\t' => saida.push(' '),
            '\\' | '`' | '*' | '_' | '[' | ']' | '<' | '>' | '|' | '~' | '&' => {
                saida.push('\\');
                saida.push(c);
            }
            _ => saida.push(c),
        }
    }
    saida
}

/// Escapa um título de aba: como `escapar`, e um `#` inicial também vira literal (senão vira cabeçalho).
pub(crate) fn escapar_titulo(texto: &str) -> String {
    let base = escapar(texto);
    if base.starts_with('#') {
        format!("\\{base}")
    } else {
        base
    }
}
