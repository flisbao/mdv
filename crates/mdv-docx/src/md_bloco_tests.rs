use crate::agrupar::{agrupar, Grupo};
use crate::md_bloco::grupo;
use crate::md_notas::Notas;
use crate::modelo::{Bloco, Celula, Marcas, Tabela, Trecho};

fn texto(s: &str) -> Vec<Trecho> {
    vec![Trecho::Texto { texto: s.into(), marcas: Marcas::default(), link: None }]
}

fn item(nivel: u8, s: &str) -> Bloco {
    Bloco::Item { nivel, numerada: false, numero: 0, texto: texto(s) }
}

fn numerado(nivel: u8, numero: u32, s: &str) -> Bloco {
    Bloco::Item { nivel, numerada: true, numero, texto: texto(s) }
}

/// Escreve todos os grupos dos blocos, um por vez, separados por linha vazia.
fn md(blocos: &[Bloco]) -> String {
    let mut notas = Notas::novo();
    let partes: Vec<String> = agrupar(blocos).iter().map(|g| grupo(g, &mut notas)).collect();
    partes.join("\n\n")
}

#[test]
fn titulo_nivel_2() {
    let b = [Bloco::Titulo { nivel: 2, texto: texto("Título") }];
    assert_eq!(md(&b), "## Título");
}

#[test]
fn paragrafo_comum() {
    assert_eq!(md(&[Bloco::Paragrafo(texto("olá mundo"))]), "olá mundo");
}

#[test]
fn lista_de_marcadores() {
    assert_eq!(md(&[item(0, "a"), item(0, "b")]), "- a\n- b");
}

#[test]
fn lista_aninhada_usa_quatro_espacos() {
    assert_eq!(md(&[item(0, "a"), item(1, "sub")]), "- a\n    - sub");
}

#[test]
fn lista_numerada_usa_o_numero_do_item() {
    assert_eq!(md(&[numerado(0, 3, "x")]), "3. x");
    assert_eq!(md(&[numerado(1, 1, "y")]), "1. y");
}

#[test]
fn lista_que_abre_em_nivel_fundo_comeca_na_margem() {
    // Quatro espaços no começo do grupo seriam um bloco de código indentado.
    assert_eq!(md(&[item(1, "x"), item(1, "y")]), "- x
- y");
}

#[test]
fn salto_de_nivel_desce_um_so() {
    assert_eq!(md(&[item(0, "a"), item(2, "b"), item(1, "c"), item(0, "d")]), "- a
    - b
    - c
- d");
}

#[test]
fn volta_de_nivel_fundo_para_raso_nao_deixa_recuo_velho() {
    let blocos = [item(0, "a"), item(1, "b"), item(2, "c"), item(0, "d"), item(2, "e")];
    assert_eq!(md(&blocos), "- a
    - b
        - c
- d
    - e");
}

#[test]
fn filho_de_numero_largo_comeca_na_coluna_do_texto() {
    // `100. ` tem 5 colunas: quatro espaços deixariam o filho como texto do item.
    assert_eq!(md(&[numerado(0, 100, "a"), item(1, "b")]), "100. a\n     - b");
}

#[test]
fn texto_de_item_que_parece_bloco_e_escapado() {
    assert_eq!(md(&[item(0, "# Segredo")]), "- \\# Segredo");
    assert_eq!(md(&[item(0, "> x")]), "- \\> x");
    assert_eq!(md(&[item(0, "- y")]), "- \\- y");
    assert_eq!(md(&[item(0, "---")]), "- \\---");
    assert_eq!(md(&[numerado(0, 3, "1. z")]), "3. 1\\. z");
}

#[test]
fn item_vazio_ainda_tem_marcador() {
    assert_eq!(md(&[item(0, "")]), "-");
}

#[test]
fn codigo_com_cerca() {
    assert_eq!(md(&[Bloco::Codigo("linha1\nlinha2".into())]), "```\nlinha1\nlinha2\n```");
}

#[test]
fn cerca_cresce_com_crases_no_codigo() {
    assert_eq!(md(&[Bloco::Codigo("a ``` b".into())]), "````\na ``` b\n````");
}

#[test]
fn codigos_seguidos_ficam_na_mesma_cerca() {
    let b = [Bloco::Codigo("um".into()), Bloco::Codigo("dois".into())];
    assert_eq!(md(&b), "```\num\ndois\n```");
}

#[test]
fn citacao_separa_paragrafos_com_linha_vazia_citada() {
    let b = [Bloco::Citacao(texto("a")), Bloco::Citacao(texto("b"))];
    assert_eq!(md(&b), "> a\n>\n> b");
}

#[test]
fn citacao_prefixa_cada_linha_da_quebra() {
    let trechos = vec![
        Trecho::Texto { texto: "a".into(), marcas: Marcas::default(), link: None },
        Trecho::Quebra,
        Trecho::Texto { texto: "b".into(), marcas: Marcas::default(), link: None },
    ];
    assert_eq!(md(&[Bloco::Citacao(trechos)]), "> a\\\n> b");
}

#[test]
fn tabela_delega() {
    let celula = |s: &str| Celula { paragrafos: vec![texto(s)], colunas: 1, continua: false };
    let t = Tabela { linhas: vec![vec![celula("A")], vec![celula("1")]] };
    let blocos = [Bloco::Tabela(t)];
    assert_eq!(grupo(&Grupo::Bloco(&blocos[0]), &mut Notas::novo()), "| A |\n| --- |\n| 1 |");
}
