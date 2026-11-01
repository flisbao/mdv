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

/// `numFmt` de id 164 com o código dado (aspas viram `&quot;` no atributo).
fn numfmt_164(codigo: &str) -> String {
    format!("<numFmts><numFmt numFmtId=\"164\" formatCode=\"{}\"/></numFmts>", codigo.replace('"', "&quot;"))
}

#[test]
fn xf_com_o_mesmo_codigo_longo_guardam_um_formato_so() {
    // código de 252 caracteres com prefixo e sufixo; 100 001 `xf` apontam para ele
    let p = "p".repeat(120);
    let s = "s".repeat(120);
    let codigo = format!("\"{p}\"#,##0.00\"{s}\"");
    assert!(codigo.chars().count() <= 255);
    let mut xml = numfmt_164(&codigo);
    xml.push_str("<cellXfs>");
    for _ in 0..=100_000 {
        xml.push_str("<xf numFmtId=\"164\"/>");
    }
    xml.push_str("</cellXfs>");
    let e = estilos(&xml);
    assert_eq!(e.formatos_distintos(), 1);
    assert_eq!(e.formato(99_999).aplicar(1.0, false), format!("{p}1.00{s}"));
}

#[test]
fn codigo_de_300_caracteres_e_ignorado_e_o_de_255_vale() {
    let ok = format!("0.00\"{}\"", "a".repeat(249)); // 255 caracteres
    let longo = format!("0.00\"{}\"", "a".repeat(294)); // 300 caracteres
    assert_eq!(ok.chars().count(), 255);
    assert_eq!(longo.chars().count(), 300);
    let aceito = estilos(&format!("{}<cellXfs><xf numFmtId=\"164\"/></cellXfs>", numfmt_164(&ok)));
    assert_eq!(aceito.formato(0).aplicar(1.0, false), format!("1.00{}", "a".repeat(249)));
    let ignorado = estilos(&format!("{}<cellXfs><xf numFmtId=\"164\"/></cellXfs>", numfmt_164(&longo)));
    assert_eq!(*ignorado.formato(0), Formato::Geral);
    assert_eq!(ignorado.formato(0).aplicar(1.0, false), "1");
}

#[test]
fn data_parcial_mostra_so_os_campos_do_formato() {
    assert_eq!(classificar(164, Some("mm/yyyy")).aplicar(45000.0, false), "2023-03");
    assert_eq!(classificar(165, Some("mmm/yy")).aplicar(45000.0, false), "2023-03");
    assert_eq!(classificar(166, Some("dd/mm")).aplicar(45000.0, false), "03-15");
    assert_eq!(classificar(167, Some("yyyy")).aplicar(45000.0, false), "2023");
    assert_eq!(classificar(168, Some("dd")).aplicar(45000.0, false), "15");
    assert_eq!(classificar(169, Some("dd/mm/yyyy hh:mm")).aplicar(45000.5, false), "2023-03-15 12:00");
}
