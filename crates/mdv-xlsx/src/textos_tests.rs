use crate::limites::Limites;
use crate::textos::ler;
use crate::Unlimited;

#[test]
fn junta_texto_rico_e_ignora_fonetico() {
    let xml = br#"<sst><si><t>Oi</t></si><si><r><t>A</t></r><r><t>B</t></r><rPh><t>zzz</t></rPh></si></sst>"#;
    assert_eq!(ler(xml, &Limites::default(), &Unlimited).unwrap(), ["Oi", "AB"]);
}
#[test]
fn corta_texto_longo_e_respeita_o_teto_de_textos() {
    let l = Limites { max_celula_chars: 3, max_textos: 1, ..Limites::default() };
    let xml = b"<sst><si><t>abcdef</t></si><si><t>x</t></si></sst>";
    assert_eq!(ler(xml, &l, &Unlimited).unwrap(), ["abc…", ""]);
}
