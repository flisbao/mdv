use crate::md_notas::Notas;
use crate::md_tabela::{tabela, tabela_com_teto};
use crate::modelo::{Celula, Marcas, Tabela, Trecho};
use crate::{Budget, Unlimited, MAX_COLUNAS};

fn texto(s: &str) -> Vec<Trecho> {
    vec![Trecho::Texto { texto: s.into(), marcas: Marcas::default(), link: None }]
}

fn c(s: &str) -> Celula {
    Celula { paragrafos: vec![texto(s)], colunas: 1, continua: false }
}

fn md(linhas: Vec<Vec<Celula>>) -> String {
    tabela(&Tabela { linhas }, &mut Notas::novo(), &Unlimited).unwrap()
}

#[test]
fn tabela_2x2() {
    let m = md(vec![vec![c("A"), c("B")], vec![c("1"), c("2")]]);
    assert_eq!(m, "| A | B |\n| --- | --- |\n| 1 | 2 |");
}

#[test]
fn gridspan_vira_celulas_vazias() {
    let a = Celula { colunas: 2, ..c("A") };
    let m = md(vec![vec![a, c("B")]]);
    assert_eq!(m, "| A |  | B |\n| --- | --- | --- |");
}

#[test]
fn vmerge_continua_fica_vazia() {
    let cont = Celula { paragrafos: vec![texto("some")], colunas: 1, continua: true };
    let m = md(vec![vec![c("A"), c("B")], vec![c("x"), c("y")], vec![cont, c("z")]]);
    assert_eq!(m, "| A | B |\n| --- | --- |\n| x | y |\n|  | z |");
}

#[test]
fn barra_vertical_no_texto_e_escapada() {
    let m = md(vec![vec![c("a|b")]]);
    assert_eq!(m, "| a\\|b |\n| --- |");
}

#[test]
fn linhas_curtas_sao_completadas() {
    let m = md(vec![vec![c("A"), c("B"), c("C")], vec![c("1"), c("2")]]);
    assert_eq!(m, "| A | B | C |\n| --- | --- | --- |\n| 1 | 2 |  |");
}

#[test]
fn celula_com_dois_paragrafos() {
    let dupla = Celula { paragrafos: vec![texto("p1"), vec![], texto("p2")], colunas: 1, continua: false };
    let m = md(vec![vec![dupla]]);
    assert_eq!(m, "| p1<br>p2 |\n| --- |");
}

#[test]
fn tabela_de_uma_linha_so_tem_cabecalho() {
    assert_eq!(md(vec![vec![c("A")]]), "| A |\n| --- |");
}

#[test]
fn colunas_demais_sao_cortadas_com_aviso() {
    let linha: Vec<Celula> = (0..=MAX_COLUNAS).map(|i| c(&i.to_string())).collect();
    let m = md(vec![linha]);
    assert!(m.ends_with("\n\n*(tabela cortada em 200 colunas)*"), "{m}");
    let cabecalho = m.lines().next().unwrap();
    assert_eq!(cabecalho.matches(" | ").count() + 1, MAX_COLUNAS);
    assert!(!cabecalho.contains("| 200 "));
}

#[test]
fn celula_mesclada_na_borda_e_cortada() {
    let larga = Celula { colunas: 3, ..c("A") };
    let linha: Vec<Celula> = std::iter::once(larga).chain((0..MAX_COLUNAS - 1).map(|_| c("x"))).collect();
    let m = md(vec![linha]);
    assert!(m.ends_with("*(tabela cortada em 200 colunas)*"), "{m}");
    assert_eq!(m.lines().next().unwrap().matches(" | ").count() + 1, MAX_COLUNAS);
}

#[test]
fn tabela_sem_linhas_da_string_vazia() {
    assert_eq!(md(vec![]), "");
}

/// Linhas de uma célula mesclada em `MAX_COLUNAS` colunas: poucas células de entrada, saída larga.
fn linhas_largas(n: usize) -> Vec<Vec<Celula>> {
    (0..n).map(|_| vec![Celula { colunas: MAX_COLUNAS, ..c("") }]).collect()
}

#[test]
fn tabela_cuja_saida_passa_do_teto_e_recusada_antes_de_montar() {
    // 11 linhas (com o separador) de 200 colunas ocupam ao menos 6600 bytes.
    let t = Tabela { linhas: linhas_largas(10) };
    let erro = tabela_com_teto(&t, &mut Notas::novo(), &Unlimited, 6599).expect_err("erro");
    assert!(erro.contains("grande demais"), "{erro}");
    assert!(tabela_com_teto(&t, &mut Notas::novo(), &Unlimited, 1 << 20).is_ok());
}

struct SemOrcamento;

impl Budget for SemOrcamento {
    fn check(&self) -> Result<(), String> {
        Err("orçamento esgotado".into())
    }
}

#[test]
fn orcamento_esgotado_para_a_tabela() {
    let t = Tabela { linhas: vec![vec![c("A")]] };
    let erro = tabela(&t, &mut Notas::novo(), &SemOrcamento).expect_err("erro");
    assert!(erro.contains("orçamento esgotado"), "{erro}");
}
