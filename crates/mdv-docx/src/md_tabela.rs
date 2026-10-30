//! Escreve uma tabela como tabela GFM.

use crate::md_inline::{inline, Modo};
use crate::md_notas::Notas;
use crate::modelo::{Celula, Tabela};
use crate::{Budget, MAX_COLUNAS, MAX_MARKDOWN_BYTES};

/// Escreve a tabela GFM; vazio se não houver linhas ou colunas.
///
/// A largura é a maior soma de `colunas` (`gridSpan`) entre as linhas, limitada a `MAX_COLUNAS`.
/// GFM não tem células mescladas: uma célula larga vira o texto mais `colunas - 1` células
/// vazias, a continuação de mescla vertical (`vMerge`) fica vazia e a linha curta é completada
/// com vazias. A primeira linha é o cabeçalho (GFM exige um); se algo passou do teto, um aviso
/// no fim diz que a tabela foi cortada.
pub(crate) fn tabela(t: &Tabela, notas: &mut Notas, budget: &dyn Budget) -> Result<String, String> {
    tabela_com_teto(t, notas, budget, MAX_MARKDOWN_BYTES)
}

/// `tabela` com o teto de bytes da saída como parâmetro (os testes usam um teto pequeno).
///
/// Cada célula ocupa ao menos 3 bytes (` | `) em cada linha, mesmo vazia, e uma linha de um
/// `<w:tc/>` com `gridSpan` de 200 vira 200 células: poucos KiB de entrada viram centenas de MiB
/// de saída. Por isso o tamanho mínimo é conferido antes de montar qualquer linha.
pub(crate) fn tabela_com_teto(t: &Tabela, notas: &mut Notas, budget: &dyn Budget, teto: usize) -> Result<String, String> {
    let soma = t.linhas.iter().map(|l| larguras(l)).max().unwrap_or(0);
    let largura = soma.min(MAX_COLUNAS);
    if largura == 0 {
        return Ok(String::new());
    }
    let minimo = (t.linhas.len() + 1).saturating_mul(largura).saturating_mul(3);
    if minimo > teto {
        return Err("documento convertido grande demais".into());
    }
    let mut linhas: Vec<String> = Vec::with_capacity(t.linhas.len() + 1);
    for l in &t.linhas {
        budget.check()?;
        linhas.push(linha(l, largura, notas));
    }
    linhas.insert(1.min(linhas.len()), separador(largura));
    let mut saida = linhas.join("\n");
    if soma > MAX_COLUNAS {
        saida.push_str(&format!("\n\n*(tabela cortada em {MAX_COLUNAS} colunas)*"));
    }
    Ok(saida)
}

/// Colunas que a linha ocupa (célula sem `colunas` conta como 1).
fn larguras(linha: &[Celula]) -> usize {
    linha.iter().fold(0usize, |soma, c| soma.saturating_add(c.colunas.max(1)))
}

/// Linha `| a | b |` com exatamente `largura` células.
fn linha(celulas: &[Celula], largura: usize, notas: &mut Notas) -> String {
    let mut textos: Vec<String> = Vec::with_capacity(largura);
    for celula in celulas {
        if textos.len() >= largura {
            break;
        }
        textos.push(texto(celula, notas));
        let vazias = celula.colunas.max(1) - 1;
        let sobra = largura - textos.len();
        textos.extend(std::iter::repeat_n(String::new(), vazias.min(sobra)));
    }
    textos.resize(largura, String::new());
    format!("| {} |", textos.join(" | "))
}

/// Linha `| --- | --- |` do cabeçalho.
fn separador(largura: usize) -> String {
    format!("| {} |", vec!["---"; largura].join(" | "))
}

/// Conteúdo da célula: parágrafos não vazios unidos por `<br>`; continuação de mescla fica vazia.
fn texto(celula: &Celula, notas: &mut Notas) -> String {
    if celula.continua {
        return String::new();
    }
    let paragrafos: Vec<String> = celula
        .paragrafos
        .iter()
        .map(|p| inline(p, notas, Modo::Celula))
        .filter(|s| !s.is_empty())
        .collect();
    paragrafos.join("<br>")
}
