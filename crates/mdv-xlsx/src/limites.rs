//! Tetos de leitura de uma planilha. `Default` traz os valores de produção; os testes usam tetos pequenos.

use crate::MAX_MARKDOWN_BYTES;

/// Tetos que limitam a memória e o tamanho da saída; passou de qualquer um, a conversão corta e,
/// em geral, avisa na saída (`max_celula_chars` deixa só `…` no texto e `max_textos` deixa a célula
/// vazia, sem aviso). Os tetos que recusam (erro) ou descartam em silêncio são constantes em `lib.rs`.
#[derive(Debug, Clone, PartialEq)]
pub struct Limites {
    /// Quantas abas visíveis entram na saída.
    pub max_abas: usize,
    /// Quantas linhas não vazias são guardadas por aba.
    pub max_linhas: usize,
    /// Quantas colunas são guardadas por aba (coluna além disso é descartada).
    pub max_colunas: usize,
    /// Tamanho máximo, em caracteres, de um texto de célula (o excedente vira `…`).
    pub max_celula_chars: usize,
    /// Quantos textos compartilhados são guardados; os demais viram vazios.
    pub max_textos: usize,
    /// Bytes de célula que a planilha inteira pode consumir (célula vazia conta 1).
    pub max_bytes_celulas: usize,
    /// Teto de células (linhas × colunas) da grade de uma aba; passou, cortam-se linhas.
    pub max_celulas: usize,
    /// Bytes de Markdown da saída inteira. A aba que faria passar do teto não entra, e a saída
    /// termina com um aviso (nunca erro).
    pub max_markdown_bytes: usize,
}

impl Default for Limites {
    fn default() -> Self {
        Limites {
            max_abas: 64,
            max_linhas: 10_000,
            max_colunas: 200,
            max_celula_chars: 2000,
            max_textos: 500_000,
            max_bytes_celulas: 8 * 1024 * 1024,
            max_celulas: 500_000,
            max_markdown_bytes: MAX_MARKDOWN_BYTES,
        }
    }
}

/// Bytes de texto de célula ainda disponíveis para a planilha inteira (compartilhado entre as abas).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Orcamento(usize);

impl Orcamento {
    pub(crate) fn novo(bytes: usize) -> Self {
        Orcamento(bytes)
    }

    /// Desconta `custo` se couber; senão devolve `false` e não muda nada.
    pub(crate) fn gastar(&mut self, custo: usize) -> bool {
        match self.0.checked_sub(custo) {
            Some(resta) => {
                self.0 = resta;
                true
            }
            None => false,
        }
    }

    /// Bytes que ainda podem ser gastos.
    #[cfg(test)]
    pub(crate) fn restante(&self) -> usize {
        self.0
    }
}
