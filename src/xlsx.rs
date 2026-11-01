//! Planilhas `.xlsx` do Excel: leitura com teto e conversão para Markdown (crate `mdv-xlsx`).
//! O texto devolvido já é Markdown (uma tabela por aba visível), então o resto do `mdv` (TUI,
//! `--plain`, `-o .md`) o trata como qualquer documento.

use std::fs;
use std::io;
use std::path::Path;

use mdv_xlsx::{Budget, MAX_XLSX_BYTES};

use crate::{load, mem};

/// Extensão `.xlsx` (sem diferenciar caixa).
pub(crate) fn is_xlsx(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("xlsx"))
}

/// O limite de memória do processo (`mem`) visto pelo `mdv-xlsx`.
struct XlsxBudget;

impl Budget for XlsxBudget {
    fn check(&self) -> Result<(), String> {
        mem::check().map_err(|e| e.to_string())
    }
}

/// Lê um `.xlsx` regular, recusando o que passa de `MAX_XLSX_BYTES` antes de ler, e devolve o
/// Markdown da planilha (uma tabela por aba visível). Erro de conversão vira `io::Error` com a
/// mensagem em português.
pub(crate) fn read_xlsx_file(path: &Path) -> io::Result<String> {
    let meta = fs::metadata(path)?;
    if !meta.is_file() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "não é um arquivo regular"));
    }
    if meta.len() > MAX_XLSX_BYTES {
        let teto = MAX_XLSX_BYTES / (1024 * 1024);
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("arquivo .xlsx grande demais (máximo {teto} MiB)"),
        ));
    }
    mem::check_input(meta.len()).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
    let bytes = load::read_limited_bytes(fs::File::open(path)?, MAX_XLSX_BYTES)?;
    mdv_xlsx::to_markdown(&bytes, &XlsxBudget).map_err(io::Error::other)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mdv_xlsx::testutil::XlsxBuilder;
    use std::fs::File;

    /// Texto das linhas renderizadas, uma por linha, sem estilos.
    fn texto_renderizado(md: &str) -> String {
        crate::render::render(md, 60)
            .lines
            .iter()
            .map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn extensao_xlsx_sem_caixa() {
        assert!(is_xlsx(Path::new("A.XLSX")));
        assert!(is_xlsx(Path::new("pasta/a.xlsx")));
        assert!(!is_xlsx(Path::new("a.xls")));
        assert!(!is_xlsx(Path::new("xlsx")));
        assert!(!is_xlsx(Path::new("a.xlsx.md")));
    }

    #[test]
    fn le_xlsx_do_disco() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("planilha.xlsx");
        let linhas = r#"<row r="1"><c r="A1"><v>7</v></c></row>"#;
        fs::write(&p, XlsxBuilder::novo().aba("P", linhas).construir()).unwrap();
        let md = read_xlsx_file(&p).unwrap();
        assert!(md.contains("# P"), "{md}");
        assert!(md.contains("| 7 |"), "{md}");
    }

    #[test]
    fn xlsx_acima_do_teto_e_recusado_sem_ler_tudo() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("grande.xlsx");
        File::create(&p).unwrap().set_len(MAX_XLSX_BYTES + 1).unwrap();
        let err = read_xlsx_file(&p).unwrap_err();
        assert!(err.to_string().contains("grande demais"), "{err}");
        assert!(err.to_string().contains("máximo 32 MiB"), "{err}");
    }

    #[test]
    fn diretorio_nao_e_arquivo_regular() {
        let d = tempfile::tempdir().unwrap();
        let err = read_xlsx_file(d.path()).unwrap_err();
        assert!(err.to_string().contains("não é um arquivo regular"), "{err}");
    }

    #[test]
    fn xlsx_exportado_pelo_proprio_mdv_volta_como_tabela() {
        use mdv_export::{convert, Context, Format, Input, PictureProvider, Select, Unlimited};
        use std::sync::Arc;

        /// Sem imagens: a entrada é CSV, nunca pede figuras.
        struct SemImagens;
        impl PictureProvider for SemImagens {
            fn load(&self, _src: &str) -> Result<Arc<image::DynamicImage>, String> {
                Err("sem imagens".to_string())
            }
            fn mermaid(&self, _source: &str) -> Option<Result<Arc<image::DynamicImage>, String>> {
                None
            }
        }

        let entrada = Input::Table { text: "nome,qtd\nlápis,3\n", delimiter: b',', no_header: false, name: "Dados" };
        let saida = convert(
            &entrada,
            &Select::default(),
            Format::Xlsx,
            &Context { budget: &Unlimited, pictures: &SemImagens },
        )
        .unwrap();
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("ida.xlsx");
        fs::write(&p, &saida.bytes).unwrap();
        let md = read_xlsx_file(&p).unwrap();
        assert!(md.contains("| nome | qtd |"), "{md}");
        assert!(md.contains("| lápis | 3 |"), "{md}");
    }

    #[test]
    fn celula_com_markup_aparece_literal_depois_de_renderizada() {
        let texto = "a|b *c* [d](e) &lt;b&gt;";
        let linhas = format!(r#"<row r="1"><c r="A1" t="inlineStr"><is><t>{texto}</t></is></c></row>"#);
        let md = mdv_xlsx::to_markdown(&XlsxBuilder::novo().aba("P", &linhas).construir(), &mdv_xlsx::Unlimited).unwrap();
        let todo = texto_renderizado(&md);
        assert!(todo.contains("a|b *c* [d](e) <b>"), "{todo}");
        // Uma só coluna: a linha com o texto tem exatamente duas barras verticais de caixa (bordas).
        assert!(
            todo.lines().any(|l| l.contains("a|b *c* [d](e) <b>") && l.matches('│').count() == 2),
            "{todo}"
        );
    }

    #[test]
    fn aba_sem_nome_ou_com_jogo_da_velha_renderiza_sem_panico() {
        let linhas = concat!(
            r#"<row r="1"><c r="A1" t="inlineStr"><is><t>x</t></is></c></row>"#,
            r#"<row r="2"><c r="A2" t="inlineStr"><is><t>y</t></is></c></row>"#,
        );
        let arquivo = XlsxBuilder::novo().aba("", linhas).aba("#", linhas).construir();
        let md = mdv_xlsx::to_markdown(&arquivo, &mdv_xlsx::Unlimited).unwrap();
        // Título vazio sai como `# ` (uma vez); o `#` vira `\#`, para não ser lido como cabeçalho.
        assert_eq!(md.lines().filter(|l| *l == "# ").count(), 1, "{md}");
        assert!(md.lines().any(|l| l == "# \\#"), "{md}");
        let todo = texto_renderizado(&md);
        // A TUI desenha a tabela com caixas: a célula aparece entre `│`.
        assert!(todo.contains("│ x │"), "{todo}");
        assert!(todo.contains("│ y │"), "{todo}");
    }
}
