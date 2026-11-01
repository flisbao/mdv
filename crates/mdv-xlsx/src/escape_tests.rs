use crate::escape::escapar;

#[test]
fn escapa_sintaxe_e_quebras() {
    assert_eq!(escapar("a|b *c* [d](e) <b> x_y"), r"a\|b \*c\* \[d\](e) \<b\> x\_y");
    assert_eq!(escapar("l1\nl2\tz"), "l1 l2 z");
}
