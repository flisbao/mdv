//! Medidas CSS mínimas (`12px`, `1.5rem`, `50%`, `auto`) e conversão para células e linhas do terminal.

/// Comprimento CSS já interpretado. `em` e `rem` valem 16 px; `ch` vale 8 px.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Len {
    Px(f32),
    Percent(f32),
    Auto,
}

/// Interpreta um comprimento. `None` para `calc()`, `vw`, `vh`, números sem unidade (exceto `0`) e unidades desconhecidas.
pub fn parse_len(s: &str) -> Option<Len> {
    let t = s.trim().to_ascii_lowercase();
    if t == "auto" {
        return Some(Len::Auto);
    }
    let fim = t
        .find(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-' || c == '+'))
        .unwrap_or(t.len());
    let numero = t.get(..fim)?;
    let unidade = t.get(fim..)?;
    let n: f32 = numero.parse().ok()?;
    if !n.is_finite() {
        return None;
    }
    match unidade {
        "px" => Some(Len::Px(n)),
        "em" | "rem" => Some(Len::Px(n * 16.0)),
        "ch" => Some(Len::Px(n * 8.0)),
        "%" => Some(Len::Percent(n)),
        "" if n == 0.0 => Some(Len::Px(0.0)),
        _ => None,
    }
}

/// Converte px em colunas de terminal (8 px por coluna), limitado a 0..=200.
pub fn px_to_cols(px: f32) -> usize {
    if !px.is_finite() {
        return 0;
    }
    (px / 8.0).round().clamp(0.0, 200.0) as usize
}

/// Converte px em linhas de terminal (16 px por linha), limitado a 0..=50.
pub fn px_to_rows(px: f32) -> usize {
    if !px.is_finite() {
        return 0;
    }
    (px / 16.0).round().clamp(0.0, 50.0) as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unidades_basicas() {
        assert_eq!(parse_len("12px"), Some(Len::Px(12.0)));
        assert_eq!(parse_len("1.5rem"), Some(Len::Px(24.0)));
        assert_eq!(parse_len("2em"), Some(Len::Px(32.0)));
        assert_eq!(parse_len("2ch"), Some(Len::Px(16.0)));
        assert_eq!(parse_len("0"), Some(Len::Px(0.0)));
        assert_eq!(parse_len("50%"), Some(Len::Percent(50.0)));
        assert_eq!(parse_len("auto"), Some(Len::Auto));
    }

    #[test]
    fn nao_reconhecidos_sao_none() {
        assert_eq!(parse_len("calc(1px + 2px)"), None);
        assert_eq!(parse_len("10vw"), None);
        assert_eq!(parse_len("nan"), None);
        assert_eq!(parse_len("12"), None);
        assert_eq!(parse_len(""), None);
    }

    #[test]
    fn colunas_e_linhas() {
        assert_eq!(px_to_cols(680.0), 85);
        assert_eq!(px_to_cols(-5.0), 0);
        assert_eq!(px_to_cols(1e9), 200);
        assert_eq!(px_to_cols(f32::NAN), 0);
        assert_eq!(px_to_rows(32.0), 2);
        assert_eq!(px_to_rows(1e9), 50);
    }
}
