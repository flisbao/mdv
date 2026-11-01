use crate::estilos::Estilos;
use crate::formato::Formato;
use crate::formato_codigo::classificar;
use crate::Unlimited;

fn estilos(xml: &str) -> Estilos {
    Estilos::carregar(xml.as_bytes(), &Unlimited).expect("styles.xml válido")
}

#[test]
#[allow(clippy::approx_constant)] // 3.14159 é o valor do brief, não a constante pi
fn aplica_formatos() {
    assert_eq!(Formato::Geral.aplicar(0.1 + 0.2, false), "0.3");
    assert_eq!(Formato::Geral.aplicar(1234567.0, false), "1234567");
    assert_eq!(Formato::Geral.aplicar(1.5e-7, false), "1.5E-07");
    assert_eq!(Formato::Geral.aplicar(123456789012345678.0, false), "1.23456789012346E+17");
    assert_eq!(classificar(2, None).aplicar(3.14159, false), "3.14");
    assert_eq!(classificar(4, None).aplicar(1234567.891, false), "1,234,567.89");
    assert_eq!(classificar(2, None).aplicar(-5.0, false), "-5.00");
    assert_eq!(classificar(9, None).aplicar(0.256, false), "26%");
    assert_eq!(classificar(10, None).aplicar(0.256, false), "25.60%");
    assert_eq!(classificar(14, None).aplicar(45000.0, false), "2023-03-15");
    assert_eq!(classificar(22, None).aplicar(45000.5, false), "2023-03-15 12:00");
    assert_eq!(classificar(21, None).aplicar(0.75, false), "18:00:00");
    assert_eq!(classificar(164, Some("\"R$\" #,##0.00")).aplicar(1234.5, false), "R$ 1,234.50");
    assert_eq!(classificar(165, Some("[$R$-416] #,##0.00")).aplicar(2.0, false), "R$ 2.00");
    assert_eq!(classificar(166, Some("dd/mm/yyyy")).aplicar(45000.0, false), "2023-03-15");
    assert_eq!(classificar(167, Some("0.00\" m²\"")).aplicar(2.5, false), "2.50 m²");
    assert_eq!(classificar(168, Some("General")).aplicar(7.0, false), "7");
    assert_eq!(classificar(12, None).aplicar(0.5, false), "0.5"); // fração: cai em Geral
    assert_eq!(classificar(11, None).aplicar(1500.0, false), "1500"); // científico: cai em Geral
    assert!(matches!(classificar(49, None), Formato::Texto));
}

#[test]
fn valor_nao_finito_nao_entra_em_formato() {
    assert_eq!(Formato::Geral.aplicar(f64::NAN, false), "");
    assert_eq!(classificar(14, None).aplicar(f64::INFINITY, false), "");
    assert_eq!(classificar(4, None).aplicar(f64::NAN, false), "");
}

#[test]
fn carregar_liga_estilo_ao_formato() {
    let e = estilos(
        "<numFmts count=\"1\"><numFmt numFmtId=\"164\" formatCode=\"0.0%\"/></numFmts>\
         <cellXfs count=\"2\"><xf numFmtId=\"0\"/><xf numFmtId=\"164\"/></cellXfs>",
    );
    assert_eq!(e.formato(1).aplicar(0.1234, false), "12.3%");
    assert_eq!(*e.formato(0), Formato::Geral);
    assert_eq!(*e.formato(99), Formato::Geral);
}

#[test]
fn carregar_ignora_xf_alem_do_teto() {
    // 100 001 `xf`: só os 100 000 primeiros entram (~2 MiB de XML de teste)
    let mut xml = String::with_capacity(2_200_000);
    xml.push_str("<cellXfs>");
    for _ in 0..=100_000 {
        xml.push_str("<xf numFmtId=\"14\"/>");
    }
    xml.push_str("</cellXfs>");
    let e = estilos(&xml);
    assert_eq!(e.formato(99_999).aplicar(45000.0, false), "2023-03-15");
    assert_eq!(*e.formato(100_000), Formato::Geral);
}
