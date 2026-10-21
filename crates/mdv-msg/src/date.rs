//! Datas e cabeçalhos RFC 5322 do e-mail: leitura de um cabeçalho de transporte e formatação das datas.

const DIAS: [&str; 7] = ["mon", "tue", "wed", "thu", "fri", "sat", "sun"];
const MESES: [&str; 12] = ["jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec"];
/// Segundos entre 1601-01-01 (FILETIME) e 1970-01-01 (Unix).
const EPOCA_FILETIME: i64 = 11_644_473_600;

/// Valor de um cabeçalho (nome sem diferenciar caixa), juntando as linhas de continuação com espaço.
pub(crate) fn header(headers: &str, name: &str) -> Option<String> {
    let mut partes: Option<Vec<&str>> = None;
    for linha in headers.split('\n') {
        let linha = linha.trim_end_matches('\r');
        if linha.starts_with([' ', '\t']) {
            if let Some(p) = partes.as_mut() {
                p.push(linha.trim());
            }
            continue;
        }
        if partes.is_some() {
            break;
        }
        if let Some((nome, valor)) = linha.split_once(':') {
            if nome.trim().eq_ignore_ascii_case(name) {
                partes = Some(vec![valor.trim()]);
            }
        }
    }
    partes
        .map(|p| p.into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" "))
        .filter(|v| !v.is_empty())
}

/// Converte uma data RFC 2822 para `AAAA-MM-DD HH:MM (±HH:MM)`, mantendo a hora do cabeçalho.
pub(crate) fn from_rfc2822(v: &str) -> Option<String> {
    let mut p: Vec<&str> = v.split_whitespace().collect();
    if let Some(primeiro) = p.first() {
        let dia = primeiro.trim_end_matches(',').to_ascii_lowercase();
        if DIAS.contains(&dia.as_str()) {
            p.remove(0);
        }
    }
    if p.len() < 5 {
        return None;
    }
    let dia: u32 = p[0].parse().ok()?;
    let mes = mes_de(p[1])?;
    let ano_bruto: i64 = p[2].parse().ok()?;
    // RFC 2822 aceita anos de dois dígitos (obsoletos): 00-49 viram 20xx, 50-99 viram 19xx.
    let ano = match ano_bruto {
        0..=49 if p[2].len() == 2 => ano_bruto + 2000,
        50..=99 if p[2].len() == 2 => ano_bruto + 1900,
        _ => ano_bruto,
    };
    if dia == 0 || dia > dias_no_mes(ano, mes) {
        return None;
    }
    let horario: Vec<&str> = p[3].split(':').collect();
    if !(2..=3).contains(&horario.len()) {
        return None;
    }
    let hora: u32 = horario[0].parse().ok()?;
    let minuto: u32 = horario[1].parse().ok()?;
    if hora > 23 || minuto > 59 {
        return None;
    }
    let (sinal, minutos_fuso) = fuso(p[4])?;
    let (oh, om) = (minutos_fuso.abs() / 60, minutos_fuso.abs() % 60);
    Some(format!("{ano:04}-{mes:02}-{dia:02} {hora:02}:{minuto:02} ({sinal}{oh:02}:{om:02})"))
}

/// Fuso do cabeçalho: `+HHMM`/`-HHMM` ou um nome antigo da RFC 822. Devolve o sinal e os minutos com sinal.
fn fuso(z: &str) -> Option<(char, i64)> {
    if let Some(num) = z.strip_prefix(['+', '-']) {
        if num.len() != 4 || !num.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        let h: i64 = num[..2].parse().ok()?;
        let m: i64 = num[2..].parse().ok()?;
        if h > 23 || m > 59 {
            return None;
        }
        let negativo = z.starts_with('-');
        let total = h * 60 + m;
        return Some(if negativo { ('-', -total) } else { ('+', total) });
    }
    let minutos: i64 = match z.to_ascii_lowercase().as_str() {
        "ut" | "gmt" | "utc" | "z" => 0,
        "est" => -300,
        "edt" => -240,
        "cst" => -360,
        "cdt" => -300,
        "mst" => -420,
        "mdt" => -360,
        "pst" => -480,
        "pdt" => -420,
        _ => return None,
    };
    Some((if minutos < 0 { '-' } else { '+' }, minutos))
}

fn mes_de(nome: &str) -> Option<u32> {
    let n = nome.to_ascii_lowercase();
    MESES.iter().position(|m| n.starts_with(m)).map(|i| i as u32 + 1)
}

fn dias_no_mes(ano: i64, mes: u32) -> u32 {
    match mes {
        2 if (ano % 4 == 0 && ano % 100 != 0) || ano % 400 == 0 => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// Converte um FILETIME (100 ns desde 1601 UTC) para `AAAA-MM-DD HH:MM UTC`.
pub(crate) fn from_filetime(ft: u64) -> String {
    let segundos = (ft / 10_000_000) as i64 - EPOCA_FILETIME;
    let (dias, resto) = (segundos.div_euclid(86_400), segundos.rem_euclid(86_400));
    let (ano, mes, dia) = data_civil(dias);
    format!("{ano:04}-{mes:02}-{dia:02} {:02}:{:02} UTC", resto / 3600, (resto % 3600) / 60)
}

/// Dias desde 1970-01-01 para (ano, mês, dia) no calendário gregoriano (algoritmo civil_from_days).
fn data_civil(dias: i64) -> (i64, u32, u32) {
    let z = dias + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let dia = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let mes = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    let ano = yoe + era * 400 + i64::from(mes <= 2);
    (ano, mes, dia)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_do_cabecalho_com_fuso() {
        assert_eq!(from_rfc2822("Tue, 21 Oct 2026 14:32:10 -0300").as_deref(), Some("2026-10-21 14:32 (-03:00)"));
    }

    #[test]
    fn data_sem_dia_da_semana() {
        assert_eq!(from_rfc2822("21 Oct 2026 09:05:00 +0000").as_deref(), Some("2026-10-21 09:05 (+00:00)"));
    }

    #[test]
    fn cabecalho_dobrado_e_juntado() {
        assert_eq!(header("Subject: a\r\n b\r\nDate: x", "subject").as_deref(), Some("a b"));
    }

    #[test]
    fn filetime_vira_utc() {
        assert_eq!(from_filetime(134_370_775_200_000_000), "2026-10-21 17:32 UTC");
    }

    #[test]
    fn data_invalida_devolve_none() {
        assert_eq!(from_rfc2822("não é data"), None);
        assert_eq!(from_rfc2822("31 Feb 2026 10:00:00 +0000"), None);
        assert_eq!(from_rfc2822("21 Oct 2026 25:00:00 +0000"), None);
    }
}
