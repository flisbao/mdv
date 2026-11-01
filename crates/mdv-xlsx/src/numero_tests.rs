use crate::numero::{fixo, geral};

#[test]
fn geral_regra_de_quinze_digitos() {
    assert_eq!(geral(0.1 + 0.2), "0.3");
    assert_eq!(geral(1234567.0), "1234567");
    assert_eq!(geral(1.5e-7), "1.5E-07");
    assert_eq!(geral(123456789012345678.0), "1.23456789012346E+17");
    assert_eq!(geral(0.0001), "0.0001");
    assert_eq!(geral(-2.5), "-2.5");
    assert_eq!(geral(0.0), "0");
    assert_eq!(geral(1e308), "1E+308");
}

#[test]
fn geral_nao_finito_vira_vazio() {
    assert_eq!(geral(f64::NAN), "");
    assert_eq!(geral(f64::INFINITY), "");
    assert_eq!(geral(f64::NEG_INFINITY), "");
}

#[test]
#[allow(clippy::approx_constant)] // 3.14159 é o valor do brief, não a constante pi
fn fixo_casas_milhar_e_arredondamento() {
    assert_eq!(fixo(3.14159, 2, false), "3.14");
    assert_eq!(fixo(1234567.891, 2, true), "1,234,567.89");
    assert_eq!(fixo(-5.0, 2, false), "-5.00");
    assert_eq!(fixo(25.6, 0, false), "26");
    assert_eq!(fixo(0.5, 0, false), "1"); // meia para longe de zero
    assert_eq!(fixo(2.675, 2, false), "2.68");
    assert_eq!(fixo(0.05, 2, false), "0.05");
    assert_eq!(fixo(-0.001, 2, false), "0.00"); // zero não ganha sinal
}

#[test]
fn fixo_limites_nao_estouram() {
    assert_eq!(fixo(f64::NAN, 2, true), "");
    assert_eq!(fixo(f64::INFINITY, 2, true), "");
    // decimais pedidas (200) são limitadas a 15; o texto cabe em poucas centenas de bytes
    let grande = fixo(1e308, 200, true);
    assert!(grande.len() < 500, "{} bytes", grande.len());
    assert!(grande.starts_with("100,000,"));
    assert!(grande.ends_with(".000000000000000"));
}
