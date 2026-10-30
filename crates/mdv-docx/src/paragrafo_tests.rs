//! Testes de `ler_paragrafo`: runs, links, revisões e elementos transparentes ou pulados.

use crate::contexto::com_contexto;
use crate::modelo::{Bloco, Marcas, Trecho};
use crate::paragrafo::ler_xml;

fn bloco(corpo: &str) -> Option<Bloco> {
    com_contexto("", "", |ctx| ler_xml(ctx, corpo))
}

fn texto(s: &str) -> Trecho {
    Trecho::Texto { texto: s.into(), marcas: Marcas::default(), link: None }
}

fn com_link(s: &str, url: &str) -> Trecho {
    Trecho::Texto { texto: s.into(), marcas: Marcas::default(), link: Some(url.into()) }
}

fn paragrafo(corpo: &str) -> Vec<Trecho> {
    match bloco(corpo) {
        Some(Bloco::Paragrafo(t)) => t,
        outro => panic!("esperava parágrafo, veio {outro:?}"),
    }
}

#[test]
fn texto_de_varios_runs() {
    let t = paragrafo("<w:r><w:t>a</w:t></w:r><w:r><w:t>b</w:t></w:r>");
    assert_eq!(t, vec![texto("a"), texto("b")]);
}

#[test]
fn hyperlink_externo_aplica_a_url() {
    let t = paragrafo(r#"<w:hyperlink r:id="rId5"><w:r><w:t>site</w:t></w:r></w:hyperlink><w:r><w:t>!</w:t></w:r>"#);
    assert_eq!(t, vec![com_link("site", "https://exemplo.org"), texto("!")]);
}

#[test]
fn hyperlink_so_com_anchor_nao_vira_link() {
    let t = paragrafo(r#"<w:hyperlink w:anchor="marca"><w:r><w:t>ir</w:t></w:r></w:hyperlink>"#);
    assert_eq!(t, vec![texto("ir")]);
}

#[test]
fn hyperlink_com_relacao_inexistente_nao_vira_link() {
    let t = paragrafo(r#"<w:hyperlink r:id="rId99"><w:r><w:t>x</w:t></w:r></w:hyperlink>"#);
    assert_eq!(t, vec![texto("x")]);
}

#[test]
fn hyperlink_com_relacao_interna_nao_vira_link() {
    // `rId7` aponta para `media/a.png`, sem `TargetMode="External"`: o alvo não pode virar URL.
    let t = paragrafo(r#"<w:hyperlink r:id="rId7"><w:r><w:t>x</w:t></w:r></w:hyperlink>"#);
    assert_eq!(t, vec![texto("x")]);
}

#[test]
fn ins_dentro_do_hyperlink_mantem_o_link() {
    let t = paragrafo(
        r#"<w:hyperlink r:id="rId5"><w:ins><w:r><w:t>a</w:t></w:r></w:ins><w:del><w:r><w:delText>b</w:delText></w:r></w:del></w:hyperlink>"#,
    );
    assert_eq!(t, vec![com_link("a", "https://exemplo.org")]);
}

#[test]
fn texto_apagado_w_del_some() {
    let t = paragrafo("<w:del><w:r><w:delText>velho</w:delText></w:r></w:del><w:r><w:t>novo</w:t></w:r>");
    assert_eq!(t, vec![texto("novo")]);
    let t = paragrafo("<w:moveFrom><w:r><w:t>velho</w:t></w:r></w:moveFrom><w:r><w:t>novo</w:t></w:r>");
    assert_eq!(t, vec![texto("novo")]);
}

#[test]
fn texto_inserido_w_ins_fica() {
    assert_eq!(paragrafo("<w:ins><w:r><w:t>novo</w:t></w:r></w:ins>"), vec![texto("novo")]);
    assert_eq!(paragrafo("<w:moveTo><w:r><w:t>novo</w:t></w:r></w:moveTo>"), vec![texto("novo")]);
}

#[test]
fn sdt_e_smarttag_sao_transparentes() {
    let t = paragrafo("<w:sdt><w:sdtPr/><w:sdtContent><w:r><w:t>a</w:t></w:r></w:sdtContent></w:sdt>");
    assert_eq!(t, vec![texto("a")]);
    let t = paragrafo("<w:smartTag><w:r><w:t>b</w:t></w:r></w:smartTag>");
    assert_eq!(t, vec![texto("b")]);
}

#[test]
fn fallback_de_alternatecontent_e_ignorado() {
    let t = paragrafo(
        "<mc:AlternateContent><mc:Choice><w:r><w:t>certo</w:t></w:r></mc:Choice><mc:Fallback><w:r><w:t>errado</w:t></w:r></mc:Fallback></mc:AlternateContent>",
    );
    assert_eq!(t, vec![texto("certo")]);
}

#[test]
fn bookmark_e_proof_err_sao_ignorados() {
    let t = paragrafo(
        r#"<w:bookmarkStart w:id="0" w:name="a"/><w:proofErr w:type="spellStart"/><w:r><w:t>x</w:t></w:r><w:proofErr w:type="spellEnd"/><w:bookmarkEnd w:id="0"/>"#,
    );
    assert_eq!(t, vec![texto("x")]);
}

#[test]
fn caixa_de_texto_com_paragrafo_aninhado_nao_corta_o_paragrafo() {
    // O `w:p` de dentro da caixa de texto fecha antes do `w:p` de fora; o texto depois dela fica.
    let t = paragrafo(
        r#"<w:r><w:t>antes</w:t></w:r><w:r><mc:AlternateContent><mc:Choice><w:drawing><wp:inline><a:graphic><w:txbxContent><w:p><w:r><w:t>dentro</w:t></w:r></w:p></w:txbxContent></a:graphic></wp:inline></w:drawing></mc:Choice></mc:AlternateContent></w:r><w:pict><w:txbxContent><w:p><w:r><w:t>pict</w:t></w:r></w:p></w:txbxContent></w:pict><w:r><w:t>depois</w:t></w:r>"#,
    );
    let textos: Vec<&Trecho> = t.iter().filter(|x| matches!(x, Trecho::Texto { .. })).collect();
    assert_eq!(textos, vec![&texto("antes"), &texto("depois")]);
}

#[test]
fn rpr_solto_no_paragrafo_e_pulado() {
    let t = paragrafo(r#"<w:rPr><w:b/><w:t>nao</w:t></w:rPr><w:r><w:t>sim</w:t></w:r>"#);
    assert_eq!(t, vec![texto("sim")]);
}

#[test]
fn link_perigoso_nao_vira_link_aqui_mas_a_url_segue_ate_o_escritor() {
    // O filtro de esquema é do escritor (Task de Markdown); aqui `externo` basta.
    let t = paragrafo(r#"<w:hyperlink r:id="rId6"><w:r><w:t>x</w:t></w:r></w:hyperlink>"#);
    assert_eq!(t, vec![com_link("x", "javascript:alert(1)")]);
}
