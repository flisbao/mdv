//! Conversão de documentos `.docx` do Word em Markdown.
//! Biblioteca pura: recebe bytes e devolve texto; a leitura do arquivo fica com o chamador.
//! O limite de trabalho vem por `Budget`; os tetos de tamanho e contagem são as constantes abaixo.
#![allow(dead_code)]

mod estilos;
mod le;
mod pacote;
mod partes;
mod relacoes;
mod xml_leitor;
mod zip_diretorio;
mod zip_entrada;
#[cfg(any(test, feature = "testutil"))]
pub mod testutil;

/// Tamanho máximo aceito para um arquivo `.docx` inteiro.
pub const MAX_DOCX_BYTES: u64 = 32 * 1024 * 1024;
/// Tamanho máximo de uma parte do pacote já descomprimida.
pub const MAX_PARTE_BYTES: usize = 16 * 1024 * 1024;
/// Soma máxima dos bytes de todas as partes lidas.
pub const MAX_TOTAL_BYTES: usize = 40 * 1024 * 1024;
/// Número máximo de entradas do diretório do ZIP.
pub const MAX_ENTRADAS: usize = 5000;
/// Profundidade máxima da árvore XML analisada.
pub const MAX_PROFUNDIDADE_XML: usize = 100;
/// Número máximo de colunas de uma tabela convertida.
pub const MAX_COLUNAS: usize = 200;
/// Nível máximo de recuo de uma lista.
pub const MAX_NIVEL_LISTA: u8 = 8;
/// Tamanho máximo do Markdown produzido.
pub const MAX_MARKDOWN_BYTES: usize = 16 * 1024 * 1024;

/// Orçamento de trabalho consultado pelos laços de conversão.
pub trait Budget {
    /// Confirma que ainda há orçamento; chamado por evento XML, por entrada do ZIP e por bloco.
    fn check(&self) -> Result<(), String>;
}

/// Orçamento sem limite, para testes e uso interno controlado.
pub struct Unlimited;

impl Budget for Unlimited {
    fn check(&self) -> Result<(), String> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn tamanho_dos_arquivos() {
        let pasta = concat!(env!("CARGO_MANIFEST_DIR"), "/src");
        for entrada in std::fs::read_dir(pasta).expect("pasta src") {
            let caminho = entrada.expect("entrada").path();
            if caminho.extension().is_some_and(|e| e == "rs") {
                let texto = std::fs::read_to_string(&caminho).expect("leitura");
                let linhas = texto.lines().count();
                let arquivo = caminho.display();
                assert!(linhas <= 500, "{arquivo} tem {linhas} linhas");
            }
        }
    }
}
