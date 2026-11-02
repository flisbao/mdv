use crate::texto_limitado::{texto_seguro, TextoLimitado};

fn acumulado(max: usize, trechos: &[&str]) -> (TextoLimitado, bool) {
    let mut t = TextoLimitado::novo(max);
    for trecho in trechos {
        t.acrescentar(trecho);
    }
    let cortado = t.cortado();
    (t, cortado)
}

#[test]
fn nunca_passa_de_max_mais_um() {
    let (t, cortou) = acumulado(3, &[&"x".repeat(10_000)]);
    assert!(cortou);
    let bruto = t.into_bruto();
    assert_eq!(bruto.chars().count(), 4);
    assert_eq!(texto_seguro(&bruto, 3), "xxx…");
}
#[test]
fn sem_corte_nao_avisa() {
    let (t, cortou) = acumulado(3, &["ab"]);
    assert!(!cortou);
    assert_eq!(t.into_bruto(), "ab");
}
#[test]
fn exatamente_no_teto_nao_corta() {
    let (t, cortou) = acumulado(3, &["ab", "c"]);
    assert!(!cortou);
    assert_eq!(texto_seguro(&t.into_bruto(), 3), "abc");
}
#[test]
fn junta_como_sanitize_no_texto_inteiro() {
    // \r\n partido entre dois trechos vira \n, como no texto inteiro
    let (t, _) = acumulado(100, &["ab\r", "\ncd", "\u{1}x\r"]);
    assert_eq!(texto_seguro(&t.into_bruto(), 100), mdv_text::sanitize("ab\r\ncd\u{1}x\r").as_ref());
}
#[test]
fn muitos_trechos_pequenos_param_no_teto() {
    // Um evento por caractere (texto partido por comentários XML): a contagem não é refeita.
    let (t, cortou) = acumulado(5, &vec!["a"; 10_000]);
    assert!(cortou);
    assert_eq!(t.into_bruto(), "aaaaaa");
}
