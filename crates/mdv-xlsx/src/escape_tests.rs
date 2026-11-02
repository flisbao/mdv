use crate::escape::escapar;

#[test]
fn escapa_sintaxe_e_quebras() {
    assert_eq!(escapar("a|b *c* [d](e) <b> x_y"), r"a\|b \*c\* \[d\](e) \<b\> x\_y");
    assert_eq!(escapar("l1\nl2\tz"), "l1 l2 z");
}

#[test]
fn titulo_escapa_hash_inicial_e_final() {
    use crate::escape::escapar_titulo;
    assert_eq!(escapar_titulo("Vendas #"), "Vendas \\#");
    assert_eq!(escapar_titulo("# A"), "\\# A");
    assert_eq!(escapar_titulo("#"), "\\#");
    assert_eq!(escapar_titulo("##"), "\\#\\#");
}

#[test]
fn titulo_ignora_espaco_nas_pontas_antes_de_escapar_hash() {
    use crate::escape::escapar_titulo;
    assert_eq!(escapar_titulo("Vendas # "), "Vendas \\#");
    assert_eq!(escapar_titulo("Vendas #\t"), "Vendas \\#");
    assert_eq!(escapar_titulo("  A  "), "A");
    assert_eq!(escapar_titulo("  "), "");
    assert_eq!(escapar_titulo("Ação #"), "Ação \\#");
}
