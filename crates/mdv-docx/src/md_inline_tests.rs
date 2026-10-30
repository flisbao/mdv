use crate::md_inline::{inline, Modo};
use crate::md_notas::Notas;
use crate::modelo::{Marcas, Trecho};

fn t(texto: &str, marcas: Marcas) -> Trecho {
    Trecho::Texto { texto: texto.into(), marcas, link: None }
}

fn simples(texto: &str) -> Trecho {
    t(texto, Marcas::default())
}

fn link(texto: &str, marcas: Marcas, url: &str) -> Trecho {
    Trecho::Texto { texto: texto.into(), marcas, link: Some(url.into()) }
}

fn negrito() -> Marcas {
    Marcas { negrito: true, ..Marcas::default() }
}

fn italico() -> Marcas {
    Marcas { italico: true, ..Marcas::default() }
}

fn codigo() -> Marcas {
    Marcas { codigo: true, ..Marcas::default() }
}

fn md(trechos: &[Trecho], modo: Modo) -> String {
    inline(trechos, &mut Notas::novo(), modo)
}

fn linha(trechos: &[Trecho]) -> String {
    md(trechos, Modo::Linha)
}

#[test]
fn texto_simples_escapa() {
    assert_eq!(linha(&[simples("a*b")]), "a\\*b");
}

#[test]
fn negrito_envolve_com_dois_asteriscos() {
    assert_eq!(linha(&[t("x", negrito())]), "**x**");
}

#[test]
fn italico_envolve_com_um_asterisco() {
    assert_eq!(linha(&[t("x", italico())]), "*x*");
}

#[test]
fn negrito_italico_usa_tres_asteriscos() {
    let marcas = Marcas { negrito: true, italico: true, ..Marcas::default() };
    assert_eq!(linha(&[t("x", marcas)]), "***x***");
}

#[test]
fn tachado_envolve_com_dois_tis() {
    let marcas = Marcas { tachado: true, ..Marcas::default() };
    assert_eq!(linha(&[t("x", marcas)]), "~~x~~");
}

#[test]
fn tachado_fica_por_fora_do_negrito() {
    let marcas = Marcas { tachado: true, negrito: true, ..Marcas::default() };
    assert_eq!(linha(&[t("x", marcas)]), "~~**x**~~");
}

#[test]
fn codigo_envolve_com_crase() {
    assert_eq!(linha(&[t("x", codigo())]), "`x`");
}

#[test]
fn codigo_nao_escapa_nada() {
    assert_eq!(linha(&[t("a*b_c", codigo())]), "`a*b_c`");
}

#[test]
fn codigo_com_crase_usa_cerca_maior_e_espacos() {
    assert_eq!(linha(&[t("a`b", codigo())]), "`` a`b ``");
}

#[test]
fn codigo_comecando_com_crase_ganha_espaco_de_guarda() {
    assert_eq!(linha(&[t("`a", codigo())]), "`` `a ``");
}

#[test]
fn codigo_com_sequencia_longa_de_crases() {
    assert_eq!(linha(&[t("a``b`c", codigo())]), "``` a``b`c ```");
}

#[test]
fn codigo_vence_negrito() {
    let marcas = Marcas { codigo: true, negrito: true, ..Marcas::default() };
    assert_eq!(linha(&[t("x", marcas)]), "`x`");
}

#[test]
fn espacos_nas_bordas_ficam_fora_do_negrito() {
    assert_eq!(linha(&[t("ola ", negrito()), simples("b")]), "**ola** b");
    assert_eq!(linha(&[simples("a"), t(" ola", negrito())]), "a **ola**");
    assert_eq!(linha(&[t("ola ", negrito())]), "**ola**");
}

#[test]
fn texto_so_de_espacos_nao_leva_marcadores() {
    assert_eq!(linha(&[simples("a"), t("  ", negrito()), simples("b")]), "a  b");
}

#[test]
fn runs_vizinhos_com_a_mesma_marca_se_fundem() {
    assert_eq!(linha(&[t("a", negrito()), t("b", negrito())]), "**ab**");
}

#[test]
fn runs_com_marcas_diferentes_nao_se_fundem() {
    assert_eq!(linha(&[t("a", negrito()), simples("b")]), "**a**b");
}

#[test]
fn runs_com_links_diferentes_nao_se_fundem() {
    let ts = [
        link("a", Marcas::default(), "https://a.org"),
        link("b", Marcas::default(), "https://b.org"),
    ];
    assert_eq!(linha(&ts), "[a](https://a.org)[b](https://b.org)");
}

#[test]
fn link_permitido() {
    let ts = [link("texto", Marcas::default(), "https://a.org")];
    assert_eq!(linha(&ts), "[texto](https://a.org)");
}

#[test]
fn link_perigoso_vira_so_texto() {
    let ts = [link("texto", Marcas::default(), "javascript:alert(1)")];
    assert_eq!(linha(&ts), "texto");
}

#[test]
fn link_com_negrito_dentro() {
    let ts = [link("x", negrito(), "https://a.org")];
    assert_eq!(linha(&ts), "[**x**](https://a.org)");
}

#[test]
fn link_deixa_espacos_das_bordas_de_fora() {
    let ts = [simples("a"), link(" x ", Marcas::default(), "https://a.org"), simples("b")];
    assert_eq!(linha(&ts), "a [x](https://a.org) b");
}

#[test]
fn link_codifica_o_destino() {
    let ts = [link("x", Marcas::default(), "https://a.org/a b")];
    assert_eq!(linha(&ts), "[x](https://a.org/a%20b)");
}

#[test]
fn quebra_por_modo() {
    let ts = [simples("a"), Trecho::Quebra, simples("b")];
    assert_eq!(md(&ts, Modo::Bloco), "a\\\nb");
    assert_eq!(md(&ts, Modo::Linha), "a b");
    assert_eq!(md(&ts, Modo::Celula), "a<br>b");
    assert_eq!(md(&ts, Modo::Titulo), "a b");
}

#[test]
fn quebra_no_fim_do_bloco_nao_deixa_barra_solta() {
    let ts = [simples("a"), Trecho::Quebra];
    assert_eq!(md(&ts, Modo::Bloco), "a");
}

#[test]
fn imagem_com_texto_alternativo() {
    assert_eq!(linha(&[Trecho::Imagem("gráfico".into())]), "*[imagem: gráfico]*");
}

#[test]
fn imagem_sem_texto_alternativo() {
    assert_eq!(linha(&[Trecho::Imagem(String::new())]), "*[imagem]*");
}

#[test]
fn imagem_escapa_o_texto_alternativo() {
    assert_eq!(linha(&[Trecho::Imagem("a*b".into())]), "*[imagem: a\\*b]*");
}

#[test]
fn nota_vira_referencia_numerada_na_ordem() {
    let mut notas = Notas::novo();
    let a = inline(&[Trecho::NotaRef("n7".into())], &mut notas, Modo::Linha);
    let b = inline(&[Trecho::NotaRef("n7".into())], &mut notas, Modo::Linha);
    let c = inline(&[Trecho::NotaRef("n3".into())], &mut notas, Modo::Linha);
    assert_eq!((a.as_str(), b.as_str(), c.as_str()), ("[^1]", "[^1]", "[^2]"));
}

#[test]
fn titulo_sem_negrito() {
    assert_eq!(md(&[t("x", negrito())], Modo::Titulo), "x");
}

#[test]
fn titulo_mantem_italico() {
    assert_eq!(md(&[t("x", italico())], Modo::Titulo), "*x*");
}

#[test]
fn celula_escapa_barra_vertical() {
    assert_eq!(md(&[simples("a|b")], Modo::Celula), "a\\|b");
}

#[test]
fn celula_escapa_barra_vertical_no_codigo() {
    assert_eq!(md(&[t("a|b", codigo())], Modo::Celula), "`a\\|b`");
}

#[test]
fn inicio_de_paragrafo_e_escapado_no_modo_bloco() {
    assert_eq!(md(&[simples("# nao titulo")], Modo::Bloco), "\\# nao titulo");
}

#[test]
fn inicio_de_linha_nao_e_escapado_fora_do_modo_bloco() {
    assert_eq!(md(&[simples("# nao titulo")], Modo::Linha), "# nao titulo");
}

#[test]
fn bordas_sao_aparadas_antes_do_escape_de_inicio() {
    assert_eq!(md(&[simples("    # x  ")], Modo::Bloco), "\\# x");
}

#[test]
fn trechos_vazios_dao_texto_vazio() {
    assert_eq!(md(&[], Modo::Bloco), "");
}
