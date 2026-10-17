//! Modelo das planilhas exportadas: o que há nas células, sem nada de XLSX ou ODS.

/// Limites do Excel; o ODS não tem os mesmos, mas respeitá-los mantém os dois formatos iguais.
pub const MAX_ROWS: usize = 1_048_576;
pub const MAX_COLS: usize = 16_384;
pub const MAX_CELL_CHARS: usize = 32_767;
/// Teto de abas por arquivo (um documento hostil poderia ter milhares de tabelas).
pub const MAX_SHEETS: usize = 100;

/// Uma célula: texto ou número. Nada vira fórmula, data ou moeda.
#[derive(Debug, Clone, PartialEq)]
pub enum Cell {
    Text(String),
    Number(f64),
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Sheet {
    pub name: String,
    /// A primeira linha é cabeçalho (negrito, congelada).
    pub header: bool,
    pub rows: Vec<Vec<Cell>>,
}

#[derive(Debug, Default)]
pub struct Workbook {
    pub sheets: Vec<Sheet>,
}

/// Número só se for decimal simples e der para ler de volta igual: `-12`, `3.14`. Zero à esquerda
/// (`007`, `01310-100`), expoente, `inf`, `NaN`, sinal `+`, espaços e mais de 15 dígitos (o Excel
/// perderia precisão) continuam texto.
pub fn parse_number(s: &str) -> Option<f64> {
    let digits = s.strip_prefix('-').unwrap_or(s);
    let (int, frac) = match digits.split_once('.') {
        Some((i, f)) => (i, Some(f)),
        None => (digits, None),
    };
    let all_digits = |t: &str| !t.is_empty() && t.bytes().all(|b| b.is_ascii_digit());
    if !all_digits(int) || frac.is_some_and(|f| !all_digits(f)) {
        return None;
    }
    if int.len() > 1 && int.starts_with('0') {
        return None;
    }
    if int.len() + frac.map_or(0, str::len) > 15 {
        return None;
    }
    s.parse().ok()
}

pub fn cell_from_text(s: &str) -> Cell {
    parse_number(s).map_or_else(|| Cell::Text(s.to_string()), Cell::Number)
}

/// Nome de aba válido no Excel e no LibreOffice: até 31 caracteres, sem `[ ] : * ? / \`, sem
/// apóstrofo nas pontas, não vazio e único (sem diferenciar caixa) entre os já `used`.
pub fn sheet_name(raw: &str, used: &[String]) -> String {
    let limpo: String = raw
        .chars()
        .map(|c| if "[]:*?/\\".contains(c) || c.is_control() { ' ' } else { c })
        .collect();
    let aparar = |s: &str| s.trim_matches(|c: char| c == ' ' || c == '\'').to_string();
    let base = utf16_prefix(&aparar(&limpo), 31);
    let base = aparar(&base);
    let base = if base.is_empty() { "Planilha".to_string() } else { base };
    let mut nome = base.clone();
    let mut n = 2;
    while used.iter().any(|u| u.to_lowercase() == nome.to_lowercase()) {
        let sufixo = format!(" ({n})");
        let cabe = 31 - sufixo.encode_utf16().count();
        nome = format!("{}{sufixo}", utf16_prefix(&base, cabe));
        n += 1;
    }
    nome
}

/// Maior prefixo de `s` com no máximo `max` unidades UTF-16 (o Excel mede o nome da aba assim:
/// um emoji fora do BMP conta 2), sem partir caractere.
fn utf16_prefix(s: &str, max: usize) -> String {
    let mut used = 0;
    s.chars().take_while(|c| {
        used += c.len_utf16();
        used <= max
    }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nome_da_aba_conta_unidades_utf16() {
        let titulo = format!("{}{}", "a".repeat(29), "😀😀😀");
        let nome = sheet_name(&titulo, &[]);
        assert_eq!(nome, format!("{}😀", "a".repeat(29)));
        assert!(nome.encode_utf16().count() <= 31);
        let outro = sheet_name(&titulo, std::slice::from_ref(&nome));
        assert!(outro.ends_with(" (2)") && outro.encode_utf16().count() <= 31, "{outro}");
        let emojis = sheet_name(&"😀".repeat(20), &[]);
        assert_eq!(emojis.encode_utf16().count(), 30, "15 emojis: o 16º passaria de 31");
    }

    #[test]
    fn numero_so_aceita_decimal_simples() {
        for (s, n) in [("0", 0.0), ("12", 12.0), ("-3", -3.0), ("3.25", 3.25), ("0.5", 0.5), ("-0.25", -0.25)] {
            assert_eq!(parse_number(s), Some(n), "{s}");
        }
    }

    #[test]
    fn numero_recusa_o_que_nao_e_decimal_simples() {
        for s in [
            "", "-", ".", "1.", ".5", "007", "00.5", "01310-100", "+5", "1e5", "1E5", "inf", "NaN",
            "0x10", " 5", "5 ", "1,5", "1 000", "1234567890123456", "1234567.123456789",
        ] {
            assert_eq!(parse_number(s), None, "{s:?}");
        }
    }

    #[test]
    fn quinze_digitos_ainda_e_numero() {
        assert_eq!(parse_number("123456789012345"), Some(123_456_789_012_345.0));
    }

    #[test]
    fn celula_de_texto_que_parece_formula_continua_texto() {
        for s in ["=1+1", "+1+1", "@SUM(A1)", "-cmd|' /C calc'!A0"] {
            assert_eq!(cell_from_text(s), Cell::Text(s.to_string()), "{s}");
        }
        assert_eq!(cell_from_text("-5"), Cell::Number(-5.0), "negativo simples é número");
    }

    #[test]
    fn nome_de_aba_e_sempre_valido() {
        let n = |raw: &str| sheet_name(raw, &[]);
        assert_eq!(n("Vendas"), "Vendas");
        assert_eq!(n("Vendas: 2024/Q1"), "Vendas  2024 Q1");
        assert_eq!(n("[Anexo]"), "Anexo");
        assert_eq!(n("'quoted'"), "quoted");
        assert_eq!(n("   "), "Planilha");
        assert_eq!(n("''"), "Planilha");
        assert_eq!(n(&"ab".repeat(40)).chars().count(), 31);
        assert!(!n("a\u{7}b").contains('\u{7}'));
    }

    #[test]
    fn nome_de_aba_repetido_ganha_sufixo_sem_diferenciar_caixa() {
        let mut usados = Vec::new();
        for _ in 0..3 {
            let nome = sheet_name("Vendas", &usados);
            usados.push(nome);
        }
        assert_eq!(usados, ["Vendas", "Vendas (2)", "Vendas (3)"]);
        assert_eq!(sheet_name("VENDAS", &usados), "VENDAS (4)");
    }

    #[test]
    fn sufixo_nao_estoura_31_caracteres() {
        let base = "x".repeat(31);
        let nome = sheet_name(&base, std::slice::from_ref(&base));
        assert_eq!(nome.chars().count(), 31);
        assert!(nome.ends_with(" (2)"));
    }
}
