use crate::celula::Celula;
use crate::limites::Limites;
use crate::markdown::escrever_aba;
use crate::planilha::{Aba, Corte};

/// Monta uma aba com os textos dados; `numericas[j]` marca a coluna `j` das linhas de dados (a partir da 2ª).
fn aba_de(linhas: &[&[&str]], numericas: &[bool]) -> Aba {
    let linhas: Vec<Vec<Celula>> = linhas
        .iter()
        .enumerate()
        .map(|(i, linha)| {
            linha
                .iter()
                .enumerate()
                .map(|(j, texto)| Celula {
                    texto: texto.to_string(),
                    numerica: i > 0 && numericas.get(j).copied().unwrap_or(false),
                })
                .collect()
        })
        .collect();
    Aba { linhas_total: linhas.len(), linhas, ..Aba::default() }
}

fn escrever(aba: &Aba, nome: &str) -> String {
    let mut saida = String::new();
    escrever_aba(nome, aba, &Limites::default(), &mut saida);
    saida
}

fn escrever_cortada() -> String {
    let mut aba = aba_de(&[&["a"], &["b"]], &[]);
    aba.linhas_total = 5;
    aba.colunas_omitidas = true;
    escrever(&aba, "Cortada")
}

#[test]
fn tabela_com_cabecalho_e_alinhamento_numerico() {
    let md = escrever(&aba_de(&[&["Produto", "Qtd"], &["Caneta", "3"], &["Total", "3"]], &[false, true]), "Vendas");
    assert_eq!(md, "# Vendas\n\n| Produto | Qtd |\n| --- | ---: |\n| Caneta | 3 |\n| Total | 3 |\n\n");
}

#[test]
fn aba_vazia() {
    assert_eq!(escrever(&aba_de(&[], &[]), "Nada"), "# Nada\n\n_(planilha vazia)_\n\n");
}

#[test]
fn avisos_de_corte() {
    let md = escrever_cortada();
    assert!(md.contains("_(mostrando as primeiras 2 de 5 linhas)_"), "{md}");
    assert!(md.contains("_(colunas além de 200 omitidas)_"), "{md}");
}

#[test]
fn aviso_de_corte_diz_qual_teto_foi_atingido() {
    let l = Limites { max_celulas: 4000, ..Limites::default() };
    let mut aba = aba_de(&[&["a"], &["b"]], &[]);
    let mut saida = String::new();
    aba.corte = Some(Corte::Celulas);
    escrever_aba("X", &aba, &l, &mut saida);
    assert!(saida.contains("_(planilha cortada: limite de 4000 células por aba atingido)_"), "{saida}");
    let mut saida = String::new();
    aba.corte = Some(Corte::Bytes);
    escrever_aba("X", &aba, &l, &mut saida);
    assert!(saida.contains("_(planilha cortada: limite de tamanho das células atingido)_"), "{saida}");
    assert!(!saida.contains("4000"), "{saida}");
}

#[test]
fn nome_da_aba_com_markup_e_literal() {
    assert!(escrever(&aba_de(&[], &[]), "# A|B\nC").starts_with("# \\# A\\|B C\n"));
}
