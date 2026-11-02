//! Modelo da planilha lida: o que `planilha` produz e `markdown` consome. Só dados, sem parsing
//! nem formatação; assim o escritor não depende do leitor.

/// Uma célula já com texto final (sem controles, no máximo `max_celula_chars` caracteres).
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Celula {
    pub texto: String,
    /// O valor de origem era número. Datas e percentuais também contam (são números com formato),
    /// e por isso saem alinhados à direita como os demais.
    pub numerica: bool,
}

/// Qual teto cortou a leitura da aba (o primeiro que disparou).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Corte {
    /// Orçamento de bytes de texto de células (compartilhado entre as abas).
    Bytes,
    /// Teto de células (`max_celulas`): células guardadas ou tamanho da grade; carrega o teto aplicado.
    Celulas { teto: usize },
}

/// Uma aba lida: linhas já com a mesma largura, mais o que foi cortado.
#[derive(Debug, Default)]
pub(crate) struct Aba {
    pub linhas: Vec<Vec<Celula>>,
    /// Linhas não vazias lidas, inclusive as que passaram de `max_linhas` (não guardadas).
    pub linhas_total: usize,
    /// `Some(teto)` se houve coluna descartada por passar do teto de colunas (`max_colunas`).
    pub colunas_omitidas: Option<usize>,
    pub corte: Option<Corte>,
}
