use crate::markdown::{escrever_aba, escrever_abas_cortadas, escrever_markdown_cortado, escrever_sem_abas_visiveis};
use crate::modelo::{Aba, Celula, Corte};

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
    escrever_aba(nome, aba, &mut saida);
    saida
}

fn escrever_cortada() -> String {
    let mut aba = aba_de(&[&["a"], &["b"]], &[]);
    aba.linhas_total = 5;
    aba.colunas_omitidas = Some(200);
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
    let mut aba = aba_de(&[&["a"], &["b"]], &[]);
    let mut saida = String::new();
    aba.corte = Some(Corte::Celulas { teto: 4000 });
    escrever_aba("X", &aba, &mut saida);
    assert!(saida.contains("_(planilha cortada: limite de 4000 células por aba atingido)_"), "{saida}");
    let mut saida = String::new();
    aba.corte = Some(Corte::Bytes);
    escrever_aba("X", &aba, &mut saida);
    assert!(saida.contains("_(planilha cortada: limite de tamanho das células atingido)_"), "{saida}");
    assert!(!saida.contains("4000"), "{saida}");
}

#[test]
fn nome_da_aba_com_markup_e_literal() {
    assert!(escrever(&aba_de(&[], &[]), "# A|B\nC").starts_with("# \\# A\\|B C\n"));
}

#[test]
fn avisos_de_planilha_inteira() {
    let mut saida = String::new();
    escrever_sem_abas_visiveis(&mut saida);
    assert_eq!(saida, "_(a planilha não tem abas visíveis)_\n");
    let mut saida = String::new();
    escrever_abas_cortadas(1, 2, &mut saida);
    assert_eq!(saida, "_(planilha cortada: 1 de 2 abas mostradas)_\n");
}

#[test]
fn aviso_de_markdown_cortado_usa_mib_quando_inteiro() {
    let mut saida = String::new();
    escrever_markdown_cortado(16 * 1024 * 1024, &mut saida);
    assert_eq!(saida, "_(planilha cortada: limite de 16 MiB de Markdown atingido)_\n");
    let mut saida = String::new();
    escrever_markdown_cortado(30, &mut saida);
    assert_eq!(saida, "_(planilha cortada: limite de 30 bytes de Markdown atingido)_\n");
}
