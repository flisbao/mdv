//! `Aba` → bloco Markdown: título `#`, tabela GFM (a primeira linha é o cabeçalho) e avisos de corte.
//! Colunas só de números saem alinhadas à direita (`---:`). Linhas vazias não chegam aqui.
//! Toda a marcação Markdown da saída, avisos inclusive, é decidida aqui.

use crate::escape::{escapar, escapar_titulo};
use crate::modelo::{Aba, Celula, Corte};

const MIB: usize = 1024 * 1024;

/// Escreve a aba em `saida`: `# nome`, tabela (ou `_(planilha vazia)_`), avisos e linha em branco.
pub(crate) fn escrever_aba(nome: &str, aba: &Aba, saida: &mut String) {
    titulo(nome, saida);
    if aba.linhas.is_empty() {
        saida.push_str(&marca_aviso("planilha vazia"));
        saida.push('\n');
    } else {
        escrever_tabela(&aba.linhas, saida);
    }
    if aba.linhas_total > aba.linhas.len() {
        aviso(saida, &format!("mostrando as primeiras {} de {} linhas", aba.linhas.len(), aba.linhas_total));
    }
    if let Some(teto) = aba.colunas_omitidas {
        aviso(saida, &format!("colunas além de {teto} omitidas"));
    }
    match aba.corte {
        Some(Corte::Celulas { teto }) => {
            aviso(saida, &format!("planilha cortada: limite de {teto} células por aba atingido"));
        }
        Some(Corte::Bytes) => aviso(saida, "planilha cortada: limite de tamanho das células atingido"),
        None => {}
    }
    saida.push('\n');
}

/// Aba cujo arquivo não está no pacote: título e aviso, sem tabela (não é uma aba vazia).
pub(crate) fn escrever_ausente(nome: &str, saida: &mut String) {
    titulo(nome, saida);
    saida.push_str(&marca_aviso("aba não encontrada no arquivo"));
    saida.push_str("\n\n");
}

/// Planilha sem nenhuma aba visível: a saída inteira é este aviso.
pub(crate) fn escrever_sem_abas_visiveis(saida: &mut String) {
    saida.push_str(&marca_aviso("a planilha não tem abas visíveis"));
    saida.push('\n');
}

/// Aviso de que só `mostradas` das `total` abas visíveis entraram (teto de abas).
pub(crate) fn escrever_abas_cortadas(mostradas: usize, total: usize, saida: &mut String) {
    saida.push_str(&marca_aviso(&format!("planilha cortada: {mostradas} de {total} abas mostradas")));
    saida.push('\n');
}

/// Aviso de que o Markdown parou no teto de `teto_bytes` bytes. O teto sai em MiB quando é um
/// número inteiro deles (o de produção, 16 MiB); senão, em bytes.
pub(crate) fn escrever_markdown_cortado(teto_bytes: usize, saida: &mut String) {
    let teto = if teto_bytes.is_multiple_of(MIB) { format!("{} MiB", teto_bytes / MIB) } else { format!("{teto_bytes} bytes") };
    saida.push_str(&marca_aviso(&format!("planilha cortada: limite de {teto} de Markdown atingido")));
    saida.push('\n');
}

/// `# nome` seguido de uma linha em branco; o nome é escapado para não virar marcação.
fn titulo(nome: &str, saida: &mut String) {
    saida.push_str("# ");
    saida.push_str(&escapar_titulo(nome));
    saida.push_str("\n\n");
}

/// Avisos são em itálico entre parênteses: `_(texto)_`.
fn marca_aviso(texto: &str) -> String {
    format!("_({texto})_")
}

/// Aviso em linha própria, separado do que vem antes por uma linha em branco (senão vira linha da tabela).
fn aviso(saida: &mut String, texto: &str) {
    saida.push('\n');
    saida.push_str(&marca_aviso(texto));
    saida.push('\n');
}

/// Tabela GFM: cabeçalho, separador (com `:` à direita nas colunas numéricas) e uma linha por registro.
fn escrever_tabela(linhas: &[Vec<Celula>], saida: &mut String) {
    let Some((cabecalho, dados)) = linhas.split_first() else { return };
    let alinhar = alinhamento_numerico(dados, cabecalho.len());
    linha_tabela(cabecalho.iter().map(|celula| escapar(&celula.texto)), saida);
    linha_tabela(alinhar.iter().map(|&direita| if direita { "---:" } else { "---" }), saida);
    for linha in dados {
        linha_tabela(linha.iter().map(|celula| escapar(&celula.texto)), saida);
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

/// Por coluna: `true` se existe ao menos uma célula não vazia nas linhas de dados e todas são
/// numéricas. "Numérica" é a marca `Celula::numerica`, que inclui data e percentual: também saem à direita.
fn alinhamento_numerico(dados: &[Vec<Celula>], colunas: usize) -> Vec<bool> {
    (0..colunas)
        .map(|coluna| {
            let mut ha_dado = false;
            for celula in dados.iter().filter_map(|linha| linha.get(coluna)).filter(|celula| !celula.texto.is_empty()) {
                if !celula.numerica {
                    return false;
                }
                ha_dado = true;
            }
            ha_dado
        })
        .collect()
}
