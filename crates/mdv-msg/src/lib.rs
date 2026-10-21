//! Conversão de e-mails `.msg` do Outlook em Markdown.
//! Biblioteca pura: recebe bytes e devolve texto; a leitura do arquivo fica com o chamador.
//! O limite de trabalho vem por `Budget`; os tetos de tamanho e contagem são as constantes abaixo.

#[allow(dead_code)]
mod codepage;
#[allow(dead_code)]
mod container;
#[allow(dead_code)]
mod escape;
#[allow(dead_code)]
mod html;
#[allow(dead_code)]
mod html_lista;
#[allow(dead_code)]
mod html_resposta;
#[allow(dead_code)]
mod html_table;
#[allow(dead_code)]
mod lzfu;
#[allow(dead_code)]
mod message;
#[allow(dead_code)]
mod rtf;
#[cfg(any(test, feature = "testutil"))]
pub mod testutil;

/// Tamanho máximo aceito para um arquivo `.msg` inteiro.
pub const MAX_MSG_BYTES: u64 = 32 * 1024 * 1024;
/// Tamanho máximo do corpo RTF (já descomprimido).
pub const MAX_RTF_BYTES: usize = 16 * 1024 * 1024;
/// Tamanho máximo do HTML extraído do corpo.
pub const MAX_HTML_BYTES: usize = 4 * 1024 * 1024;
/// Tamanho máximo do Markdown produzido.
pub const MAX_MARKDOWN_BYTES: usize = 16 * 1024 * 1024;
/// Profundidade máxima de mensagens anexadas dentro de mensagens.
pub const MAX_EMBED_DEPTH: usize = 3;
/// Número máximo de destinatários listados.
pub const MAX_RECIPIENTS: usize = 500;
/// Número máximo de anexos listados.
pub const MAX_ATTACHMENTS: usize = 200;
/// Profundidade máxima da árvore HTML analisada.
pub const MAX_HTML_DEPTH: usize = 100;
/// Número máximo de colunas de uma tabela convertida.
pub const MAX_TABLE_COLS: usize = 200;
/// Número máximo de caracteres por célula de tabela.
pub const MAX_CELL_CHARS: usize = 500;

/// Orçamento de trabalho consultado pelos laços de conversão.
pub trait Budget {
    /// Confirma que ainda há orçamento; chamado a cada iteração de bloco, linha ou nó.
    fn check(&self) -> Result<(), String>;
    /// Barra, antes de analisar, um HTML cuja árvore passaria do limite.
    fn check_html(&self, len: usize) -> Result<(), String>;
}

/// Orçamento sem limite, para testes e uso interno controlado.
pub struct Unlimited;

impl Budget for Unlimited {
    fn check(&self) -> Result<(), String> {
        Ok(())
    }

    fn check_html(&self, _len: usize) -> Result<(), String> {
        Ok(())
    }
}
