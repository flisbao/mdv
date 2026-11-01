use crate::referencia::coluna_de;

#[test]
fn coluna_de_referencias() {
    assert_eq!(coluna_de("A1"), Some(0));
    assert_eq!(coluna_de("Z9"), Some(25));
    assert_eq!(coluna_de("AA1"), Some(26));
    assert_eq!(coluna_de("XFD1048576"), Some(16383));
    assert_eq!(coluna_de("1"), None);
    assert_eq!(coluna_de(""), None);
}
#[test]
fn coluna_gigante_nao_estoura() {
    assert!(coluna_de(&format!("{}1", "Z".repeat(40))).is_some());
}
