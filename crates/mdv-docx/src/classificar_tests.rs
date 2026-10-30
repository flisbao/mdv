//! Testes de `classificar`, exercitados pela leitura de um parágrafo completo.

use crate::contexto::com_contexto;
use crate::modelo::{Bloco, Marcas, Trecho};
use crate::paragrafo::ler_xml;

const ESTILOS: &str = r#"
    <w:style w:styleId="Normal"><w:name w:val="Normal"/></w:style>
    <w:style w:styleId="Heading2"><w:name w:val="heading 2"/></w:style>
    <w:style w:styleId="Citacao"><w:name w:val="Quote"/></w:style>
    <w:style w:styleId="Codigo"><w:name w:val="Source Code"/></w:style>
    <w:style w:styleId="ListBullet"><w:name w:val="List Bullet"/><w:pPr><w:numPr><w:ilvl w:val="1"/><w:numId w:val="1"/></w:numPr></w:pPr></w:style>
    <w:style w:styleId="TituloNum"><w:name w:val="heading 1"/><w:pPr><w:numPr><w:numId w:val="2"/></w:numPr></w:pPr></w:style>"#;

const NUMERACAO: &str = r#"
    <w:abstractNum w:abstractNumId="0"><w:lvl w:ilvl="0"><w:numFmt w:val="bullet"/></w:lvl><w:lvl w:ilvl="1"><w:numFmt w:val="bullet"/></w:lvl></w:abstractNum>
    <w:abstractNum w:abstractNumId="1"><w:lvl w:ilvl="0"><w:numFmt w:val="decimal"/></w:lvl><w:lvl w:ilvl="8"><w:numFmt w:val="decimal"/></w:lvl></w:abstractNum>
    <w:num w:numId="1"><w:abstractNumId w:val="0"/></w:num>
    <w:num w:numId="2"><w:abstractNumId w:val="1"/></w:num>"#;

fn bloco(ppr: &str, runs: &str) -> Option<Bloco> {
    let corpo = format!("<w:pPr>{ppr}</w:pPr>{runs}");
    com_contexto(ESTILOS, NUMERACAO, |ctx| ler_xml(ctx, &corpo))
}

const RUN: &str = "<w:r><w:t>texto</w:t></w:r>";

fn texto(s: &str) -> Vec<Trecho> {
    vec![Trecho::Texto { texto: s.into(), marcas: Marcas::default(), link: None }]
}

#[test]
fn titulo_pelo_estilo() {
    assert_eq!(
        bloco(r#"<w:pStyle w:val="Heading2"/>"#, RUN),
        Some(Bloco::Titulo { nivel: 2, texto: texto("texto") })
    );
}

#[test]
fn outline_9_direto_tira_o_titulo_do_estilo() {
    assert_eq!(
        bloco(r#"<w:pStyle w:val="Heading2"/><w:outlineLvl w:val="9"/>"#, RUN),
        Some(Bloco::Paragrafo(texto("texto")))
    );
}

#[test]
fn titulo_por_outline_direto_vence_o_estilo() {
    assert_eq!(
        bloco(r#"<w:pStyle w:val="Normal"/><w:outlineLvl w:val="0"/>"#, RUN),
        Some(Bloco::Titulo { nivel: 1, texto: texto("texto") })
    );
    assert_eq!(
        bloco(r#"<w:pStyle w:val="Heading2"/><w:outlineLvl w:val="3"/>"#, RUN),
        Some(Bloco::Titulo { nivel: 4, texto: texto("texto") })
    );
}

#[test]
fn item_de_marcador() {
    assert_eq!(
        bloco(r#"<w:numPr><w:numId w:val="1"/></w:numPr>"#, RUN),
        Some(Bloco::Item { nivel: 0, numerada: false, numero: 0, texto: texto("texto") })
    );
}

#[test]
fn item_numerado_conta() {
    let corpo = r#"<w:pPr><w:numPr><w:numId w:val="2"/></w:numPr></w:pPr><w:r><w:t>x</w:t></w:r>"#;
    let numeros = com_contexto(ESTILOS, NUMERACAO, |ctx| {
        [ler_xml(ctx, corpo), ler_xml(ctx, corpo)]
            .map(|b| match b {
                Some(Bloco::Item { numerada: true, numero, .. }) => numero,
                outro => panic!("esperava item numerado, veio {outro:?}"),
            })
    });
    assert_eq!(numeros, [1, 2]);
}

#[test]
fn numid_zero_nao_e_lista() {
    assert_eq!(
        bloco(r#"<w:numPr><w:numId w:val="0"/></w:numPr>"#, RUN),
        Some(Bloco::Paragrafo(texto("texto")))
    );
}

#[test]
fn numid_zero_direto_cancela_a_lista_do_estilo() {
    assert_eq!(
        bloco(r#"<w:pStyle w:val="ListBullet"/><w:numPr><w:ilvl w:val="0"/><w:numId w:val="0"/></w:numPr>"#, RUN),
        Some(Bloco::Paragrafo(texto("texto")))
    );
}

#[test]
fn item_pelo_numpr_do_estilo() {
    assert_eq!(
        bloco(r#"<w:pStyle w:val="ListBullet"/>"#, RUN),
        Some(Bloco::Item { nivel: 1, numerada: false, numero: 0, texto: texto("texto") })
    );
}

#[test]
fn nivel_da_lista_e_limitado_ao_teto() {
    let b = bloco(r#"<w:numPr><w:ilvl w:val="200"/><w:numId w:val="2"/></w:numPr>"#, RUN);
    assert_eq!(b, Some(Bloco::Item { nivel: 8, numerada: true, numero: 1, texto: texto("texto") }));
    let b = bloco(r#"<w:numPr><w:ilvl w:val="99999"/><w:numId w:val="1"/></w:numPr>"#, RUN);
    assert_eq!(b, Some(Bloco::Item { nivel: 8, numerada: false, numero: 0, texto: texto("texto") }));
}

#[test]
fn codigo_pelo_estilo() {
    let runs = "<w:r><w:t>a</w:t><w:br/><w:t>b</w:t></w:r>";
    assert_eq!(bloco(r#"<w:pStyle w:val="Codigo"/>"#, runs), Some(Bloco::Codigo("a\nb".into())));
}

#[test]
fn codigo_quando_todos_os_runs_sao_mono() {
    let mono = r#"<w:rPr><w:rFonts w:ascii="Courier New"/></w:rPr>"#;
    let runs = format!("<w:r>{mono}<w:t>x = 1</w:t><w:br/><w:t>y</w:t></w:r><w:r>{mono}<w:t>;</w:t></w:r>");
    assert_eq!(bloco("", &runs), Some(Bloco::Codigo("x = 1\ny;".into())));
}

#[test]
fn run_comum_junto_do_mono_nao_vira_codigo() {
    let mono = r#"<w:rPr><w:rFonts w:ascii="Courier New"/></w:rPr>"#;
    let runs = format!("<w:r>{mono}<w:t>x</w:t></w:r><w:r><w:t>y</w:t></w:r>");
    assert!(matches!(bloco("", &runs), Some(Bloco::Paragrafo(_))));
}

#[test]
fn imagem_e_nota_no_codigo_sao_ignoradas() {
    let runs = r#"<w:r><w:t>a</w:t><w:footnoteReference w:id="2"/></w:r>"#;
    let b = bloco(r#"<w:pStyle w:val="Codigo"/>"#, runs);
    assert_eq!(b, Some(Bloco::Codigo("a".into())));
}

#[test]
fn citacao_pelo_estilo() {
    assert_eq!(
        bloco(r#"<w:pStyle w:val="Citacao"/>"#, RUN),
        Some(Bloco::Citacao(texto("texto")))
    );
}

#[test]
fn paragrafo_vazio_some() {
    assert_eq!(bloco("", "<w:r><w:t>   </w:t></w:r>"), None);
    assert_eq!(bloco("", ""), None);
    assert_eq!(bloco("", "<w:r><w:br/></w:r>"), None);
}

#[test]
fn paragrafo_so_com_imagem_ou_nota_nao_some() {
    let b = bloco("", r#"<w:r><w:drawing><wp:inline><wp:docPr id="1" descr="foto"/></wp:inline></w:drawing></w:r>"#);
    assert_eq!(b, Some(Bloco::Paragrafo(vec![Trecho::Imagem("foto".into())])));
    let b = bloco("", r#"<w:r><w:footnoteReference w:id="2"/></w:r>"#);
    assert_eq!(b, Some(Bloco::Paragrafo(vec![Trecho::NotaRef("n2".into())])));
}

#[test]
fn titulo_vence_lista() {
    assert_eq!(
        bloco(r#"<w:pStyle w:val="TituloNum"/><w:numPr><w:numId w:val="2"/></w:numPr>"#, RUN),
        Some(Bloco::Titulo { nivel: 1, texto: texto("texto") })
    );
}

#[test]
fn lista_vence_codigo_e_citacao() {
    let b = bloco(r#"<w:pStyle w:val="Citacao"/><w:numPr><w:numId w:val="1"/></w:numPr>"#, RUN);
    assert!(matches!(b, Some(Bloco::Item { .. })));
}
