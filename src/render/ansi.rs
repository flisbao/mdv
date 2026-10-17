//! Conversão das linhas renderizadas para SGR (modo `--plain`).

use super::*;

/// Converte linhas estilizadas em texto com escapes ANSI (ou texto puro se `color` for falso).
pub fn to_ansi(lines: &[Line<'_>], color: bool) -> String {
    let mut out = String::new();
    for line in lines {
        for span in &line.spans {
            let opened = color && write_sgr(&mut out, &span.style);
            out.push_str(&span.content);
            if opened {
                out.push_str("\x1b[0m");
            }
        }
        out.push('\n');
    }
    out
}

/// Abre a sequência SGR do estilo em `out`; devolve `false` (sem escrever nada) se o estilo é vazio.
pub(super) fn write_sgr(out: &mut String, st: &Style) -> bool {
    const MODIFIERS: [(Modifier, &str); 5] = [
        (Modifier::BOLD, "1"),
        (Modifier::DIM, "2"),
        (Modifier::ITALIC, "3"),
        (Modifier::UNDERLINED, "4"),
        (Modifier::CROSSED_OUT, "9"),
    ];
    let start = out.len();
    out.push_str("\x1b[");
    let params_start = out.len();
    let param = |out: &mut String, args: std::fmt::Arguments<'_>| {
        if out.len() > params_start {
            out.push(';');
        }
        // Escrever em `String` não falha.
        let _ = out.write_fmt(args);
    };
    for (modifier, code) in MODIFIERS {
        if st.add_modifier.contains(modifier) {
            param(out, format_args!("{code}"));
        }
    }
    for (color, base) in [(st.fg, 38), (st.bg, 48)] {
        match color {
            Some(Color::Rgb(r, g, b)) => param(out, format_args!("{base};2;{r};{g};{b}")),
            Some(Color::Indexed(i)) => param(out, format_args!("{base};5;{i}")),
            _ => {}
        }
    }
    if out.len() == params_start {
        out.truncate(start);
        return false;
    }
    out.push('m');
    true
}
