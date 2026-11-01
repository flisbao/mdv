use crate::celula::{converter, Celula};
use crate::formato::Formato;

fn texto(c: &Celula) -> &str {
    &c.texto
}

#[test]
fn tipo_s_usa_o_texto_compartilhado() {
    let textos = vec!["a".to_string(), "b".to_string()];
    let c = converter(Some("s"), "1", &Formato::Geral, &textos, false);
    assert_eq!((texto(&c), c.numerica), ("b", false));
}
#[test]
fn tipo_s_fora_do_intervalo_vira_vazio() {
    let textos = vec!["a".to_string()];
    let c = converter(Some("s"), "9", &Formato::Geral, &textos, false);
    assert_eq!(texto(&c), "");
}
#[test]
fn tipo_str_e_texto_cru() {
    let c = converter(Some("str"), "abc", &Formato::Geral, &[], false);
    assert_eq!((texto(&c), c.numerica), ("abc", false));
}
#[test]
fn tipo_b_vira_verdadeiro_ou_falso() {
    assert_eq!(texto(&converter(Some("b"), "1", &Formato::Geral, &[], false)), "VERDADEIRO");
    assert_eq!(texto(&converter(Some("b"), "0", &Formato::Geral, &[], false)), "FALSO");
}
#[test]
fn tipo_e_mostra_o_erro() {
    let c = converter(Some("e"), "#DIV/0!", &Formato::Geral, &[], false);
    assert_eq!((texto(&c), c.numerica), ("#DIV/0!", false));
}
#[test]
fn tipo_d_fica_como_esta() {
    assert_eq!(texto(&converter(Some("d"), "2024-01-02", &Formato::Geral, &[], false)), "2024-01-02");
}
#[test]
fn tipo_inline_str_fica_como_esta() {
    assert_eq!(texto(&converter(Some("inlineStr"), "ola", &Formato::Geral, &[], false)), "ola");
}
#[test]
fn tipo_n_usa_o_formato_e_e_numerica() {
    let c = converter(Some("n"), "0.30000000000000004", &Formato::Geral, &[], false);
    assert_eq!((texto(&c), c.numerica), ("0.3", true));
    let p = Formato::Percentual { decimais: 0 };
    assert_eq!(texto(&converter(None, "0.5", &p, &[], false)), "50%");
}
#[test]
fn valor_vazio_vira_celula_vazia() {
    let c = converter(Some("n"), "", &Formato::Geral, &[], false);
    assert_eq!(c, Celula::default());
}
#[test]
fn numero_invalido_mostra_o_texto() {
    let c = converter(None, "abc", &Formato::Geral, &[], false);
    assert_eq!((texto(&c), c.numerica), ("abc", false));
}
