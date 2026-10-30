//! Escreve uma tabela como tabela GFM.

use crate::limite::Limite;
use crate::md_inline::{inline, Modo};
use crate::md_notas::Notas;
use crate::modelo::{Celula, Tabela};
use crate::MAX_COLUNAS;

/// Escreve a tabela GFM; vazio se não houver linhas ou colunas.
///
/// A largura é a maior soma de `colunas` (`gridSpan`) entre as linhas, limitada a `MAX_COLUNAS`.
/// GFM não tem células mescladas: uma célula larga vira o texto mais `colunas - 1` células
/// vazias, a continuação de mescla vertical (`vMerge`) fica vazia e a linha curta é completada
/// com vazias. A primeira linha é o cabeçalho (GFM exige um); se algo passou do teto, um aviso
/// no fim diz que a tabela foi cortada.
///
/// Uma linha de um `<w:tc/>` com `gridSpan` de 200 vira 200 células: poucos KiB de entrada viram
/// centenas de MiB de saída. Por isso cada célula, mesmo vazia, desconta 3 bytes (` | `) do
/// `limite` antes de ser montada, e a tabela para na primeira linha que estourar.
pub(crate) fn tabela(t: &Tabela, notas: &mut Notas, limite: &mut Limite) -> Result<String, String> {
    let soma = t.linhas.iter().map(|l| larguras(l)).max().unwrap_or(0);
    let largura = soma.min(MAX_COLUNAS);
    if largura == 0 {
        return Ok(String::new());
    }
    let mut linhas: Vec<String> = Vec::with_capacity(t.linhas.len() + 1);
    for l in &t.linhas {
        linhas.push(linha(l, largura, notas, limite)?);
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
fn linha(celulas: &[Celula], largura: usize, notas: &mut Notas, limite: &mut Limite) -> Result<String, String> {
    let mut textos: Vec<String> = Vec::with_capacity(largura);
    for celula in celulas {
        if textos.len() >= largura {
            break;
        }
        textos.push(texto(celula, notas, limite)?);
        let vazias = (celula.colunas.max(1) - 1).min(largura - textos.len());
        limite.gastar(3 * (1 + vazias))?;
        textos.extend(std::iter::repeat_n(String::new(), vazias));
    }
    limite.gastar(3 * (largura - textos.len()))?;
    textos.resize(largura, String::new());
    Ok(format!("| {} |", textos.join(" | ")))
}

/// Linha `| --- | --- |` do cabeçalho.
fn separador(largura: usize) -> String {
    format!("| {} |", vec!["---"; largura].join(" | "))
}

/// Conteúdo da célula: parágrafos não vazios unidos por `<br>`; continuação de mescla fica vazia.
fn texto(celula: &Celula, notas: &mut Notas, limite: &mut Limite) -> Result<String, String> {
    if celula.continua {
        return Ok(String::new());
    }
    let mut paragrafos: Vec<String> = Vec::new();
    for p in &celula.paragrafos {
        let s = inline(p, notas, Modo::Celula);
        if !s.is_empty() {
            limite.gastar(s.len() + 4)?;
            paragrafos.push(s);
        }
    }
    Ok(paragrafos.join("<br>"))
}
