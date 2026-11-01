//! Número de série do Excel → data e hora. Sistema 1900 com o defeito do ano bissexto de 1900
//! (série 60 = 1900-02-29) e sistema 1904 (`date1904`). Nada aqui aloca: só aritmética.

/// Maior série aceita (9999-12-31 no sistema 1900).
const MAX_SERIE: f64 = 2_958_465.0;
const SEGUNDOS_NO_DIA: f64 = 86_400.0;

/// Ano, mês e dia de uma série. Série 0 no sistema 1900 vira `1900-01-00`, como no Excel.
pub(crate) fn serie_para_data(serie: f64, data_1904: bool) -> Option<(i32, u32, u32)> {
    if !serie.is_finite() || !(0.0..=MAX_SERIE).contains(&serie) {
        return None;
    }
    let n = serie.floor() as i64;
    if data_1904 {
        return Some(civil(dias_desde_civil(1904, 1, 1) + n));
    }
    Some(match n {
        0 => (1900, 1, 0),
        60 => (1900, 2, 29),
        1..=59 => civil(dias_desde_civil(1899, 12, 31) + n),
        _ => civil(dias_desde_civil(1899, 12, 30) + n),
    })
}

/// Hora, minuto e segundo da parte fracionária, arredondada ao segundo. 24:00:00 volta a 0:0:0.
pub(crate) fn serie_para_hora(serie: f64) -> (u32, u32, u32) {
    if !serie.is_finite() {
        return (0, 0, 0);
    }
    let segundos = (serie.rem_euclid(1.0) * SEGUNDOS_NO_DIA).round() as u64 % SEGUNDOS_NO_DIA as u64;
    ((segundos / 3600) as u32, ((segundos % 3600) / 60) as u32, (segundos % 60) as u32)
}

/// A série com a parte fracionária arredondada ao segundo (pode subir para o dia seguinte).
pub(crate) fn arredondar_segundo(serie: f64) -> f64 {
    if !serie.is_finite() {
        return serie;
    }
    let inteira = serie.floor();
    inteira + ((serie - inteira) * SEGUNDOS_NO_DIA).round() / SEGUNDOS_NO_DIA
}

/// Dias desde 1970-01-01 (calendário civil gregoriano, sem crate de datas).
fn dias_desde_civil(ano: i64, mes: i64, dia: i64) -> i64 {
    let ano = if mes <= 2 { ano - 1 } else { ano };
    let era = ano.div_euclid(400);
    let yoe = ano - era * 400;
    let doy = (153 * (if mes > 2 { mes - 3 } else { mes + 9 }) + 2) / 5 + dia - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Inverso de `dias_desde_civil`: dias desde 1970-01-01 → (ano, mês, dia).
fn civil(dias: i64) -> (i32, u32, u32) {
    let z = dias + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let dia = doy - (153 * mp + 2) / 5 + 1;
    let mes = if mp < 10 { mp + 3 } else { mp - 9 };
    let ano = yoe + era * 400 + i64::from(mes <= 2);
    (ano as i32, mes as u32, dia as u32)
}
