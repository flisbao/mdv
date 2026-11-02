//! Conversão de planilhas `.xlsx` do Excel em Markdown.
//! Biblioteca pura: recebe bytes e devolve texto; a leitura do arquivo fica com o chamador.
//! O limite de trabalho vem por `Budget`; os tetos de tamanho e contagem são as constantes abaixo.
//! A API pública é só `to_markdown`, `Budget`, `Unlimited`, as constantes de teto e `testutil`;
//! `Limites` (tetos que cortam com aviso) e `to_markdown_com_limites` são internos à crate.

mod celula;
mod conversao;
mod data;
mod escape;
mod estilos;
mod formato;
mod formato_codigo;
mod formato_literais;
mod le;
mod limites;
mod markdown;
mod modelo;
mod numero;
mod pacote;
mod pasta;
mod planilha;
mod referencia;
mod relacoes;
mod texto_limitado;
mod textos;
mod xml_leitor;
mod zip_diretorio;
mod zip_entrada;
pub use conversao::to_markdown;
#[cfg(any(test, feature = "testutil"))]
pub mod testutil;
#[cfg(test)]
mod data_tests;
#[cfg(test)]
mod formato_tests;
#[cfg(test)]
mod numero_tests;
#[cfg(test)]
mod texto_limitado_tests;
#[cfg(test)]
mod textos_tests;
#[cfg(test)]
mod referencia_tests;
#[cfg(test)]
mod celula_tests;
#[cfg(test)]
mod pasta_tests;
#[cfg(test)]
mod planilha_tests;
#[cfg(test)]
mod escape_tests;
#[cfg(test)]
mod markdown_tests;
#[cfg(test)]
mod conversao_tests;

/// Tamanho máximo aceito para um arquivo `.xlsx` inteiro.
pub const MAX_XLSX_BYTES: u64 = 32 * 1024 * 1024;
/// Tamanho máximo de uma parte do pacote já descomprimida.
pub const MAX_PARTE_BYTES: usize = 16 * 1024 * 1024;
/// Soma máxima dos bytes de todas as partes lidas.
pub const MAX_TOTAL_BYTES: usize = 40 * 1024 * 1024;
/// Número máximo de entradas do diretório do ZIP.
pub const MAX_ENTRADAS: usize = 5000;
/// Profundidade máxima da árvore XML analisada.
pub const MAX_PROFUNDIDADE_XML: usize = 100;
/// Tamanho máximo do Markdown produzido para uma planilha.
pub const MAX_MARKDOWN_BYTES: usize = 16 * 1024 * 1024;

/// Orçamento de trabalho consultado pelos laços de conversão.
pub trait Budget {
    /// Confirma que ainda há orçamento; chamado por evento XML, por entrada do ZIP e por aba.
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
