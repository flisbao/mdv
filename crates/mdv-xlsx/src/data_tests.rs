use crate::data::{serie_para_data, serie_para_hora};

#[test]
fn series_de_referencia() {
    assert_eq!(serie_para_data(45000.0, false), Some((2023, 3, 15)));
    assert_eq!(serie_para_data(1.0, false), Some((1900, 1, 1)));
    assert_eq!(serie_para_data(59.0, false), Some((1900, 2, 28)));
    assert_eq!(serie_para_data(60.0, false), Some((1900, 2, 29))); // defeito do Excel
    assert_eq!(serie_para_data(61.0, false), Some((1900, 3, 1)));
    assert_eq!(serie_para_data(0.0, true), Some((1904, 1, 1)));
    assert_eq!(serie_para_data(-1.0, false), None);
    assert_eq!(serie_para_hora(0.5), (12, 0, 0));
    assert_eq!(serie_para_hora(45000.75), (18, 0, 0));
}

#[test]
fn serie_fora_da_faixa_ou_nao_finita_da_none() {
    assert_eq!(serie_para_data(f64::NAN, false), None);
    assert_eq!(serie_para_data(f64::INFINITY, false), None);
    assert_eq!(serie_para_data(f64::NEG_INFINITY, true), None);
    assert_eq!(serie_para_data(1e300, true), None);
    assert_eq!(serie_para_data(2_958_466.0, false), None);
    assert_eq!(serie_para_data(2_958_465.0, false), Some((9999, 12, 31)));
    assert_eq!(serie_para_hora(f64::NAN), (0, 0, 0));
}
