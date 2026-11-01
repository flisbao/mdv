//! Tetos de leitura de uma planilha. `Default` traz os valores de produção; os testes usam tetos pequenos.

/// Tetos que limitam a memória e o tamanho da saída; passou do teto, a conversão corta e avisa.
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
        }
    }
}
