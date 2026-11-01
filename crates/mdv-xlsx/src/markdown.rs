//! `Aba` → bloco Markdown: título `#`, tabela GFM (a primeira linha é o cabeçalho) e avisos de corte.
//! Colunas só de números saem alinhadas à direita (`---:`). Linhas vazias não chegam aqui.

use crate::escape::{escapar, escapar_titulo};
use crate::limites::Limites;
use crate::planilha::{Aba, Corte};

/// Escreve a aba em `saida`: `# nome`, tabela (ou `_(planilha vazia)_`), avisos e linha em branco.
pub(crate) fn escrever_aba(nome: &str, aba: &Aba, limites: &Limites, saida: &mut String) {
    saida.push_str("# ");
    saida.push_str(&escapar_titulo(nome));
    saida.push_str("\n\n");
    if aba.linhas.is_empty() {
        saida.push_str("_(planilha vazia)_\n");
    } else {
        escrever_tabela(&aba.linhas, saida);
    }
    if aba.linhas_total > aba.linhas.len() {
        aviso(saida, &format!("_(mostrando as primeiras {} de {} linhas)_", aba.linhas.len(), aba.linhas_total));
    }
    if aba.colunas_omitidas {
        aviso(saida, &format!("_(colunas além de {} omitidas)_", limites.max_colunas));
    }
    match aba.corte {
        Some(Corte::Celulas) => {
            aviso(saida, &format!("_(planilha cortada: limite de {} células por aba atingido)_", limites.max_celulas));
        }
        Some(Corte::Bytes) => aviso(saida, "_(planilha cortada: limite de tamanho das células atingido)_"),
        None => {}
    }
    saida.push('\n');
}

/// Aba cujo arquivo não está no pacote: título e aviso, sem tabela (não é uma aba vazia).
pub(crate) fn escrever_ausente(nome: &str, saida: &mut String) {
    saida.push_str("# ");
    saida.push_str(&escapar_titulo(nome));
    saida.push_str("\n\n_(aba não encontrada no arquivo)_\n\n");
}

/// Aviso em linha própria, separado do que vem antes por uma linha em branco (senão vira linha da tabela).
fn aviso(saida: &mut String, texto: &str) {
    saida.push('\n');
    saida.push_str(texto);
    saida.push('\n');
}

/// Tabela GFM: cabeçalho, separador (com `:` à direita nas colunas numéricas) e uma linha por registro.
fn escrever_tabela(linhas: &[Vec<crate::celula::Celula>], saida: &mut String) {
    let colunas = linhas[0].len();
    let alinhar = alinhamento_numerico(linhas, colunas);
    let mut cabecalho = true;
    for linha in linhas {
        linha_tabela(linha.iter().map(|c| escapar(&c.texto)), saida);
        if cabecalho {
            let separador = alinhar.iter().map(|&direita| if direita { "---:" } else { "---" });
            linha_tabela(separador, saida);
            cabecalho = false;
        }
    }
}

/// Uma linha `| a | b |`; célula vazia sai como `  ` (dois espaços entre os pipes).
fn linha_tabela<I: Iterator<Item = impl AsRef<str>>>(celulas: I, saida: &mut String) {
    saida.push('|');
    for celula in celulas {
        saida.push(' ');
        saida.push_str(celula.as_ref());
        saida.push_str(" |");
    }
    saida.push('\n');
}

/// Por coluna: `true` se existe ao menos uma célula não vazia nas linhas de dados e todas são numéricas.
fn alinhamento_numerico(linhas: &[Vec<crate::celula::Celula>], colunas: usize) -> Vec<bool> {
    (0..colunas)
        .map(|c| {
            let mut ha_dado = false;
            for celula in linhas.iter().skip(1).filter_map(|l| l.get(c)).filter(|c| !c.texto.is_empty()) {
                if !celula.numerica {
                    return false;
                }
                ha_dado = true;
            }
            ha_dado
        })
        .collect()
}
