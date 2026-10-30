//! Testes ponta a ponta de `to_markdown`: do ZIP montado à mão até o Markdown final.

use crate::testutil::{zip, DocxBuilder, NS};
use crate::{to_markdown, Budget, Unlimited, MAX_PROFUNDIDADE_XML, MAX_URL_BYTES};

fn md(corpo: &str) -> String {
    to_markdown(&DocxBuilder::novo().corpo(corpo).construir(), &Unlimited).expect("converte")
}

const ESTILOS: &str = r#"
    <w:style w:styleId="Heading1"><w:name w:val="heading 1"/></w:style>
    <w:style w:styleId="Heading2"><w:name w:val="heading 2"/></w:style>"#;

const NUMERACAO: &str = r#"
    <w:abstractNum w:abstractNumId="0">
      <w:lvl w:ilvl="0"><w:numFmt w:val="bullet"/></w:lvl>
      <w:lvl w:ilvl="1"><w:numFmt w:val="decimal"/></w:lvl>
    </w:abstractNum>
    <w:num w:numId="1"><w:abstractNumId w:val="0"/></w:num>"#;

const CORPO_BASICO: &str = r#"
    <w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>Título</w:t></w:r></w:p>
    <w:p><w:pPr><w:pStyle w:val="Heading2"/></w:pPr><w:r><w:t>Sub</w:t></w:r></w:p>
    <w:p><w:r><w:rPr><w:b/></w:rPr><w:t>forte</w:t></w:r></w:p>
    <w:p><w:r><w:rPr><w:i/></w:rPr><w:t>suave</w:t></w:r></w:p>"#;

fn basico() -> DocxBuilder {
    DocxBuilder::novo().estilos(ESTILOS).corpo(CORPO_BASICO)
}

fn item(nivel: u8, texto: &str) -> String {
    format!(
        r#"<w:p><w:pPr><w:numPr><w:ilvl w:val="{nivel}"/><w:numId w:val="1"/></w:numPr></w:pPr><w:r><w:t>{texto}</w:t></w:r></w:p>"#
    )
}

struct SemOrcamento;

impl Budget for SemOrcamento {
    fn check(&self) -> Result<(), String> {
        Err("orçamento esgotado".into())
    }
}

#[test]
fn titulos_paragrafos_e_formatacao() {
    let saida = to_markdown(&basico().construir(), &Unlimited).expect("converte");
    assert_eq!(saida, "# Título\n\n## Sub\n\n**forte**\n\n*suave*\n");
}

#[test]
fn lista_com_numbering_xml() {
    let corpo = [item(0, "a"), item(1, "b"), item(1, "c"), item(0, "d")].concat();
    let bytes = DocxBuilder::novo().numeracao(NUMERACAO).corpo(&corpo).construir();
    let saida = to_markdown(&bytes, &Unlimited).expect("converte");
    assert_eq!(saida, "- a\n    1. b\n    2. c\n- d\n");
}

#[test]
fn marcador_raso_reinicia_a_numeracao_dos_niveis_fundos() {
    let corpo = [item(0, "a"), item(1, "b"), item(1, "c"), item(0, "d"), item(1, "e")].concat();
    let bytes = DocxBuilder::novo().numeracao(NUMERACAO).corpo(&corpo).construir();
    let saida = to_markdown(&bytes, &Unlimited).expect("converte");
    assert_eq!(saida, "- a\n    1. b\n    2. c\n- d\n    1. e\n");
}

#[test]
fn lista_numerada_dentro_de_celula_nao_desloca_a_do_corpo() {
    const DECIMAL: &str = r#"<w:abstractNum w:abstractNumId="0"><w:lvl w:ilvl="0"><w:numFmt w:val="decimal"/></w:lvl></w:abstractNum>
        <w:num w:numId="1"><w:abstractNumId w:val="0"/></w:num>"#;
    let numerado = |t: &str| {
        format!(r#"<w:p><w:pPr><w:numPr><w:numId w:val="1"/></w:numPr></w:pPr><w:r><w:t>{t}</w:t></w:r></w:p>"#)
    };
    let corpo = format!(
        "{}<w:tbl><w:tr><w:tc>{}{}</w:tc></w:tr></w:tbl>{}",
        numerado("x"),
        numerado("d1"),
        numerado("d2"),
        numerado("y")
    );
    let bytes = DocxBuilder::novo().numeracao(DECIMAL).corpo(&corpo).construir();
    let saida = to_markdown(&bytes, &Unlimited).expect("converte");
    assert!(saida.starts_with("1. x\n\n"), "{saida}");
    assert!(saida.ends_with("\n\n2. y\n"), "{saida}");
}

#[test]
fn fallback_de_bloco_nao_duplica_paragrafo_nem_tabela() {
    let p = "<w:p><w:r><w:t>unico</w:t></w:r></w:p>";
    let tabela = "<w:tbl><w:tr><w:tc><w:p><w:r><w:t>cel</w:t></w:r></w:p></w:tc></w:tr></w:tbl>";
    let alterna = |dentro: &str| {
        format!("<mc:AlternateContent><mc:Choice Requires=\"x\">{dentro}</mc:Choice><mc:Fallback>{dentro}</mc:Fallback></mc:AlternateContent>")
    };
    assert_eq!(md(&alterna(p)), "unico\n");
    assert_eq!(md(&alterna(tabela)), "| cel |\n| --- |\n");
    let na_celula = format!("<w:tbl><w:tr><w:tc>{}</w:tc></w:tr></w:tbl>", alterna(p));
    assert_eq!(md(&na_celula), "| unico |\n| --- |\n");
}

#[test]
fn equacao_nao_vaza_como_texto_corrido() {
    let eq = r#"<m:oMathPara xmlns:m="http://schemas.openxmlformats.org/officeDocument/2006/math"><m:oMath><m:r><m:t>x+y</m:t></m:r></m:oMath></m:oMathPara>"#;
    let saida = md(&format!("<w:p><w:r><w:t>a</w:t></w:r>{eq}<w:r><w:t>b</w:t></w:r></w:p>"));
    assert_eq!(saida, "ab\n");
}

#[test]
fn tabela_aninhada_acima_da_profundidade_e_recusada() {
    // Cada tabela soma 3 níveis (tbl, tr, tc): 40 passam de MAX_PROFUNDIDADE_XML.
    let n = 40;
    let corpo = format!("{}<w:p/>{}", "<w:tbl><w:tr><w:tc>".repeat(n), "</w:tc></w:tr></w:tbl>".repeat(n));
    let bytes = DocxBuilder::novo().corpo(&corpo).construir();
    let erro = to_markdown(&bytes, &Unlimited).expect_err("erro");
    assert!(erro.contains("profundidade"), "{erro}");
}

#[test]
fn tabela_com_celulas_mescladas() {
    let saida = md(r#"<w:tbl>
        <w:tr><w:tc><w:tcPr><w:gridSpan w:val="2"/></w:tcPr><w:p><w:r><w:t>topo</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>c</w:t></w:r></w:p></w:tc></w:tr>
        <w:tr><w:tc><w:tcPr><w:vMerge w:val="restart"/></w:tcPr><w:p><w:r><w:t>x</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>y</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>z</w:t></w:r></w:p></w:tc></w:tr>
        <w:tr><w:tc><w:tcPr><w:vMerge/></w:tcPr><w:p/></w:tc><w:tc><w:p><w:r><w:t>w</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>v</w:t></w:r></w:p></w:tc></w:tr>
    </w:tbl>"#);
    let barras: Vec<usize> = saida.lines().map(|l| l.matches('|').count()).collect();
    assert_eq!(barras.len(), 4, "{saida}");
    assert!(barras.iter().all(|&n| n == 4), "{saida}");
    assert!(saida.contains("| topo |  | c |"), "{saida}");
}

#[test]
fn texto_apagado_nao_aparece_e_inserido_sim() {
    let saida = md(r#"<w:p><w:del><w:r><w:delText>velho</w:delText></w:r></w:del><w:ins><w:r><w:t>novo</w:t></w:r></w:ins></w:p>"#);
    assert_eq!(saida, "novo\n");
}

#[test]
fn nota_de_rodape_vira_definicao() {
    let bytes = DocxBuilder::novo()
        .corpo(r#"<w:p><w:r><w:t>texto</w:t></w:r><w:r><w:footnoteReference w:id="1"/></w:r></w:p>"#)
        .notas(r#"<w:footnote w:id="1"><w:p><w:r><w:t>conteúdo da nota</w:t></w:r></w:p></w:footnote>"#)
        .construir();
    let saida = to_markdown(&bytes, &Unlimited).expect("converte");
    assert_eq!(saida, "texto[^1]\n\n[^1]: conteúdo da nota\n");
}

#[test]
fn link_externo_e_javascript() {
    const TIPO: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink";
    let bytes = DocxBuilder::novo()
        .relacao("rId5", TIPO, "https://exemplo.org", true)
        .relacao("rId6", TIPO, "javascript:alert(1)", true)
        .corpo(
            r#"<w:p><w:hyperlink r:id="rId5"><w:r><w:t>site</w:t></w:r></w:hyperlink></w:p>
               <w:p><w:hyperlink r:id="rId6"><w:r><w:t>perigo</w:t></w:r></w:hyperlink></w:p>"#,
        )
        .construir();
    let saida = to_markdown(&bytes, &Unlimited).expect("converte");
    assert_eq!(saida, "[site](https://exemplo.org)\n\nperigo\n");
}

#[test]
fn link_com_url_acima_do_teto_vira_so_texto() {
    // A URL é copiada por run: acima do teto o texto fica, sem link.
    const TIPO: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink";
    let longa = format!("https://a.org/{}", "x".repeat(MAX_URL_BYTES));
    let curta = "https://a.org/ok";
    let bytes = DocxBuilder::novo()
        .relacao("rId5", TIPO, &longa, true)
        .relacao("rId6", TIPO, curta, true)
        .corpo(
            r#"<w:p><w:hyperlink r:id="rId5"><w:r><w:t>longo</w:t></w:r></w:hyperlink></w:p>
               <w:p><w:hyperlink r:id="rId6"><w:r><w:t>curto</w:t></w:r></w:hyperlink></w:p>"#,
        )
        .construir();
    let saida = to_markdown(&bytes, &Unlimited).expect("converte");
    assert_eq!(saida, "longo\n\n[curto](https://a.org/ok)\n");
}

#[test]
fn notas_acima_do_teto_sao_recusadas() {
    // 100 001 notas de um caractere somam cerca de 8 MiB de XML, abaixo do teto da parte.
    let notas: String = (1..=100_001)
        .map(|i| format!(r#"<w:footnote w:id="{i}"><w:p><w:r><w:t>a</w:t></w:r></w:p></w:footnote>"#))
        .collect();
    let bytes = DocxBuilder::novo().corpo("<w:p><w:r><w:t>x</w:t></w:r></w:p>").notas(&notas).construir();
    let erro = to_markdown(&bytes, &Unlimited).expect_err("erro");
    assert!(erro.contains("notas demais"), "{erro}");
}

#[test]
fn documento_vazio_avisa() {
    assert_eq!(md(""), "*(documento sem texto)*\n");
}

#[test]
fn doc_antigo_ole2_da_erro_claro() {
    let mut bytes = vec![0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];
    bytes.resize(bytes.len() + 100, 0);
    let erro = to_markdown(&bytes, &Unlimited).expect_err("erro");
    assert!(erro.contains("criptografado ou no formato .doc antigo"), "{erro}");
}

#[test]
fn lixo_da_erro_zip_invalido() {
    let erro = to_markdown(b"nao sou zip", &Unlimited).expect_err("erro");
    assert!(erro.contains("ZIP inválido"), "{erro}");
}

#[test]
fn docx_comprimido_funciona() {
    let saida = to_markdown(&basico().comprimido().construir(), &Unlimited).expect("converte");
    assert_eq!(saida, "# Título\n\n## Sub\n\n**forte**\n\n*suave*\n");
}

/// `.docx` com só `word/document.xml`, cujo XML é `xml` (a parte principal cai no nome padrão).
fn so_documento(xml: &str) -> Vec<u8> {
    zip(&[("word/document.xml", xml.as_bytes())], false)
}

#[test]
fn dtd_no_documento_e_recusado() {
    let xml = format!(
        r#"<?xml version="1.0"?><!DOCTYPE w:document [<!ENTITY a "bomba">]><w:document {NS}><w:body><w:p><w:r><w:t>&a;</w:t></w:r></w:p></w:body></w:document>"#
    );
    let erro = to_markdown(&so_documento(&xml), &Unlimited).expect_err("erro");
    assert!(erro.contains("DTD"), "{erro}");
}

#[test]
fn profundidade_de_xml_recusada() {
    let n = MAX_PROFUNDIDADE_XML + 1;
    let corpo = format!("{}{}", "<w:sdt>".repeat(n), "</w:sdt>".repeat(n));
    let xml = format!(r#"<?xml version="1.0"?><w:document {NS}><w:body>{corpo}</w:body></w:document>"#);
    let erro = to_markdown(&so_documento(&xml), &Unlimited).expect_err("erro");
    assert!(erro.contains("profundidade"), "{erro}");
}

#[test]
fn controle_de_terminal_some() {
    let saida = md("<w:p><w:r><w:t>a\u{1b}]0;titulo\u{7}b</w:t></w:r></w:p>");
    assert!(!saida.contains('\u{1b}') && !saida.contains('\u{7}'), "{saida:?}");
    assert!(saida.contains('a') && saida.contains('b'), "{saida:?}");
}

#[test]
fn orcamento_esgotado_para_a_conversao() {
    let erro = to_markdown(&basico().construir(), &SemOrcamento).expect_err("erro");
    assert!(erro.contains("orçamento esgotado"), "{erro}");
}
