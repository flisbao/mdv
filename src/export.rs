//! Ramo `-o` da linha de comando: carrega a entrada, escolhe o que exportar, chama o
//! `mdv-export` e grava o arquivo. O `mdv-export` não sabe de arquivos, memória do processo
//! nem do `ImageStore`: aqui estão os adaptadores.

use std::io::Write;
use std::path::Path;
use std::sync::Arc;

use mdv_export::{convert, Budget, Context, Format, Input, PictureProvider, Select, FORMATS_HELP};

use crate::image::{ImageState, ImageStore};
use crate::load::{image_store, load_source, Source};
use crate::plain::write_stdout;
use crate::{cli, mem, render, Failure};

/// O limite de memória do processo (`mem`), visto pelo `mdv-export`.
struct MemBudget;

impl Budget for MemBudget {
    fn check(&self) -> Result<(), String> {
        mem::check().map_err(|e| e.to_string())
    }
    fn check_parse(&self, len: usize) -> Result<(), String> {
        mem::check_parse(len).map_err(|e| e.to_string())
    }
}

/// O `ImageStore` (disco, `data:`, rede com `--remote`, Mermaid nativo) visto pelo `mdv-export`.
struct StorePictures<'a>(&'a ImageStore);

/// Recusa a imagem local cujo caminho real (links simbólicos resolvidos) sai da pasta do
/// documento: o `mdv-export` só confere a forma do caminho, e um `link.png -> /etc/...` dentro da
/// pasta levaria para o arquivo compartilhado um arquivo qualquer do computador. Usa o mesmo nome
/// que o `ImageStore` abriria (`local_name`). Arquivo inexistente segue para o erro do store.
fn confined_to_base(store: &ImageStore, src: &str) -> Result<(), String> {
    let lower = src.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") || lower.starts_with("data:") {
        return Ok(());
    }
    let base = store.base_dir().unwrap_or(Path::new("."));
    let Ok(real) = std::fs::canonicalize(base.join(store.local_name(src))) else {
        return Ok(());
    };
    let base = std::fs::canonicalize(base).map_err(|e| format!("não consegui resolver a pasta do documento: {e}"))?;
    if real.starts_with(&base) {
        Ok(())
    } else {
        Err("a imagem aponta para fora da pasta do documento (link simbólico?)".to_string())
    }
}

impl PictureProvider for StorePictures<'_> {
    fn load(&self, src: &str) -> Result<Arc<image::DynamicImage>, String> {
        confined_to_base(self.0, src)?;
        match self.0.get(src) {
            ImageState::Ready(img) => Ok(img),
            ImageState::Loading => Err("ainda carregando".to_string()),
            ImageState::Failed(e) => Err(e),
        }
    }
    fn mermaid(&self, source: &str) -> Option<Result<Arc<image::DynamicImage>, String>> {
        // Sem exigir gráficos (Kitty): o PNG vai para dentro do arquivo.
        if !self.0.mermaid_configured() {
            return None;
        }
        Some(match self.0.get_mermaid(source) {
            ImageState::Ready(img) => Ok(img),
            ImageState::Loading => Err("ainda carregando".to_string()),
            ImageState::Failed(e) => Err(e),
        })
    }
}

/// Converte `input` e grava em `out` (formato pela extensão). Devolve os avisos. Nunca deixa um
/// arquivo pela metade: grava num temporário na mesma pasta e só então o põe no lugar. Todo erro
/// de formato, destino ou combinação vem **antes** de criar qualquer coisa.
pub(crate) fn export_to_file(
    input: &Input,
    select: &Select,
    out: &Path,
    force: bool,
    store: &ImageStore,
) -> Result<Vec<String>, String> {
    let ext = out.extension().and_then(|e| e.to_str()).unwrap_or("");
    let format = Format::from_ext(ext)
        .ok_or_else(|| format!("formato de saída não suportado: .{ext} (use {FORMATS_HELP})"))?;
    check_destination(out, force)?;
    let pictures = StorePictures(store);
    let output = convert(input, select, format, &Context { budget: &MemBudget, pictures: &pictures })?;
    write_atomic(out, &output.bytes, force)?;
    Ok(output.warnings)
}

/// Checagens comuns a todo `-o`: o destino não pode ser pasta, e sem `--force` não sobrescreve.
fn check_destination(out: &Path, force: bool) -> Result<(), String> {
    if out.is_dir() {
        return Err(format!("{} é uma pasta", out.display()));
    }
    if out.exists() && !force {
        return Err(format!("{} já existe (use --force para sobrescrever)", out.display()));
    }
    Ok(())
}

/// `-o x.md` (só de `.msg`): grava o Markdown já convertido, com as mesmas checagens e a mesma
/// gravação atômica das demais saídas.
pub(crate) fn write_markdown(out: &Path, text: &str, force: bool) -> Result<(), String> {
    check_destination(out, force)?;
    write_atomic(out, text.as_bytes(), force)
}

fn write_atomic(out: &Path, bytes: &[u8], force: bool) -> Result<(), String> {
    let dir = match out.parent() {
        Some(d) if !d.as_os_str().is_empty() => d,
        _ => Path::new("."),
    };
    let fail = |e: std::io::Error| format!("não consegui gravar {}: {e}", out.display());
    let mut tmp = temp_file_in(dir).map_err(fail)?;
    tmp.write_all(bytes).map_err(fail)?;
    tmp.flush().map_err(fail)?;
    tmp.as_file().sync_all().map_err(fail)?;
    let result = if force { tmp.persist(out) } else { tmp.persist_noclobber(out) };
    result.map(|_| ()).map_err(|e| fail(e.error))
}

/// Temporário na pasta de destino. No Unix pede o modo 0666, que o `umask` de quem exporta reduz
/// (o padrão do `tempfile`, 0600, esconderia o arquivo de quem o receber; 0644 fixo ignoraria o
/// `umask` de quem quer o arquivo privado).
fn temp_file_in(dir: &Path) -> std::io::Result<tempfile::NamedTempFile> {
    let mut builder = tempfile::Builder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        builder.permissions(std::fs::Permissions::from_mode(0o666));
    }
    builder.tempfile_in(dir)
}

/// Nome da aba para CSV/TSV: o arquivo sem extensão (ou `Dados` com stdin).
fn sheet_label(source: &Source) -> String {
    match source {
        Source::File(p) => p.file_stem().and_then(|s| s.to_str()).unwrap_or("Dados").to_string(),
        Source::Stdin => "Dados".to_string(),
    }
}

/// `-o` nunca abre a TUI: o aviso de memória (`mem::warn_if_above`) vai sempre para o stderr.
pub(crate) fn run_export(cli: &cli::Cli, e: &cli::Export, mem_warning: Option<&str>) -> Result<(), Failure> {
    if let Some(w) = mem_warning {
        eprintln!("mdv: {w}");
    }
    let (source, text) = load_source(cli.common.path.as_deref())?;
    if cli::saida_markdown(&e.output) {
        // A entrada já é o Markdown do `.msg` (`load_source` converte); só falta gravar.
        write_markdown(Path::new(&e.output), &text, e.force).map_err(Failure::Runtime)?;
        return write_stdout(&format!("exportado: {}\n", render::sanitize(&e.output)));
    }
    let side = mdv_export::images::MAX_SIDE;
    let store = image_store(&source, cli, true).with_max_pixels(side, side);
    let label = sheet_label(&source);
    let (input, select) = match &e.doc {
        cli::Doc::Markdown => {
            (Input::Markdown(&text), Select { section: e.section.as_deref(), table: cli.common.table })
        }
        cli::Doc::Table { delimiter, no_header, .. } => (
            Input::Table { text: &text, delimiter: *delimiter, no_header: *no_header, name: &label },
            Select::default(),
        ),
        cli::Doc::Json { .. } => return Err(Failure::Usage("JSON ainda não exporta".to_string())),
    };
    let warnings = export_to_file(&input, &select, Path::new(&e.output), e.force, &store).map_err(Failure::Runtime)?;
    for w in warnings {
        eprintln!("mdv: aviso: {}", render::sanitize(&w));
    }
    write_stdout(&format!("exportado: {}\n", render::sanitize(&e.output)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use mdv_export::{Input, Select};

    fn store(dir: Option<&std::path::Path>) -> ImageStore {
        ImageStore::new(dir.map(std::path::Path::to_path_buf))
    }

    fn md(text: &str) -> Input<'_> {
        Input::Markdown(text)
    }

    #[test]
    fn grava_os_cinco_formatos_pela_extensao() {
        let d = tempfile::tempdir().unwrap();
        let md_tabela = "# T\n\n| a |\n|---|\n| 1 |\n";
        for (name, magic) in [("a.pdf", &b"%PDF"[..]), ("a.docx", b"PK"), ("a.odt", b"PK"), ("a.XLSX", b"PK"), ("a.ods", b"PK")] {
            let out = d.path().join(name);
            let w = export_to_file(&md(md_tabela), &Select::default(), &out, false, &store(None)).unwrap();
            assert!(w.is_empty(), "{name}: {w:?}");
            assert!(std::fs::read(&out).unwrap().starts_with(magic), "{name}");
        }
    }

    #[test]
    fn nao_sobrescreve_sem_force_e_sobrescreve_com() {
        let d = tempfile::tempdir().unwrap();
        let out = d.path().join("a.pdf");
        std::fs::write(&out, "antigo").unwrap();
        let err = export_to_file(&md("x\n"), &Select::default(), &out, false, &store(None)).unwrap_err();
        assert!(err.contains("--force"), "{err}");
        assert_eq!(std::fs::read_to_string(&out).unwrap(), "antigo", "intacto");
        export_to_file(&md("x\n"), &Select::default(), &out, true, &store(None)).unwrap();
        assert!(std::fs::read(&out).unwrap().starts_with(b"%PDF"));
    }

    #[test]
    fn formato_desconhecido_e_pasta_sao_recusados_sem_criar_nada() {
        let d = tempfile::tempdir().unwrap();
        let err = export_to_file(&md("x\n"), &Select::default(), &d.path().join("a.txt"), false, &store(None)).unwrap_err();
        for ext in [".txt", ".pdf", ".docx", ".odt", ".xlsx", ".ods"] {
            assert!(err.contains(ext), "{err}");
        }
        let sub = d.path().join("pasta.pdf");
        std::fs::create_dir(&sub).unwrap();
        assert!(export_to_file(&md("x\n"), &Select::default(), &sub, true, &store(None)).unwrap_err().contains("pasta"));
        assert_eq!(std::fs::read_dir(d.path()).unwrap().count(), 1, "só a pasta; nenhum temporário sobrou");
    }

    #[test]
    fn erro_de_conversao_nao_cria_arquivo_nem_temporario() {
        let d = tempfile::tempdir().unwrap();
        let out = d.path().join("t.xlsx");
        let e = export_to_file(&md("# so texto\n"), &Select::default(), &out, false, &store(None)).unwrap_err();
        assert!(e.contains("não tem tabelas"), "{e}");
        assert_eq!(std::fs::read_dir(d.path()).unwrap().count(), 0);
    }

    #[test]
    fn imagem_local_relativa_entra_e_absoluta_e_recusada_com_aviso() {
        let d = tempfile::tempdir().unwrap();
        image::RgbImage::new(4, 4).save(d.path().join("a.png")).unwrap();
        let out = d.path().join("o.docx");
        let w = export_to_file(&md("![ok](a.png)\n\n![ruim](/etc/passwd)\n"), &Select::default(), &out, false, &store(Some(d.path()))).unwrap();
        assert_eq!(w.len(), 1, "{w:?}");
        assert!(w[0].contains("/etc/passwd"), "{w:?}");
    }

    #[cfg(unix)]
    #[test]
    fn arquivo_final_segue_o_umask_e_nunca_e_executavel() {
        use std::os::unix::fs::PermissionsExt;
        let d = tempfile::tempdir().unwrap();
        let out = d.path().join("a.pdf");
        export_to_file(&md("x\n"), &Select::default(), &out, false, &store(None)).unwrap();
        let mode = std::fs::metadata(&out).unwrap().permissions().mode();
        assert_eq!(mode & 0o111, 0, "nunca executável: {mode:o}");
        assert_ne!(mode & 0o400, 0, "o dono lê: {mode:o}");
    }

    /// Os nomes das entradas do ZIP ficam em claro no arquivo: basta procurar o prefixo.
    fn has_entry(path: &std::path::Path, prefix: &str) -> bool {
        std::fs::read(path).unwrap().windows(prefix.len()).any(|w| w == prefix.as_bytes())
    }

    #[cfg(feature = "mermaid")]
    #[test]
    fn mermaid_vira_imagem_no_docx_e_no_odt_sem_graficos() {
        let d = tempfile::tempdir().unwrap();
        let st = store(None).with_native_mermaid(true);
        assert!(!st.mermaid_enabled(), "sem Kitty a TUI continua mostrando código");
        let doc = "```mermaid\ngraph TD\n  A --> B\n```\n";
        for (name, dir) in [("m.docx", "word/media/"), ("m.odt", "Pictures/")] {
            let out = d.path().join(name);
            let w = export_to_file(&md(doc), &Select::default(), &out, false, &st).unwrap();
            if !crate::mermaid_native::fonts_available() {
                assert!(w.iter().any(|x| x.contains("mermaid")), "sem fontes o aviso volta: {w:?}");
                continue;
            }
            assert!(w.is_empty(), "{name}: {w:?}");
            assert!(has_entry(&out, dir), "{name}");
        }
    }

    #[test]
    fn mermaid_desligado_fica_como_codigo_sem_imagem() {
        let d = tempfile::tempdir().unwrap();
        let out = d.path().join("m.odt");
        let w = export_to_file(&md("```mermaid\ngraph TD\n  A --> B\n```\n"), &Select::default(), &out, false, &store(None)).unwrap();
        assert!(w.is_empty(), "{w:?}");
        assert!(!has_entry(&out, "Pictures/"));
    }

    #[cfg(unix)]
    #[test]
    fn link_simbolico_para_fora_da_pasta_e_recusado() {
        let fora = tempfile::tempdir().unwrap();
        image::RgbImage::new(4, 4).save(fora.path().join("s.png")).unwrap();
        let d = tempfile::tempdir().unwrap();
        image::RgbImage::new(4, 4).save(d.path().join("a.png")).unwrap();
        std::os::unix::fs::symlink(fora.path().join("s.png"), d.path().join("link.png")).unwrap();
        std::os::unix::fs::symlink(d.path().join("a.png"), d.path().join("dentro.png")).unwrap();
        let st = store(Some(d.path()));
        for src in ["link.png", "%6Cink.png"] {
            let out = d.path().join("o.odt");
            let w = export_to_file(&md(&format!("![alt]({src})\n")), &Select::default(), &out, true, &st).unwrap();
            assert!(w.len() == 1 && w[0].contains("fora da pasta"), "{src}: {w:?}");
            assert!(!has_entry(&out, "Pictures/"), "{src}");
        }
        for src in ["dentro.png", "a.png"] {
            let out = d.path().join("o.odt");
            let w = export_to_file(&md(&format!("![alt]({src})\n")), &Select::default(), &out, true, &st).unwrap();
            assert!(w.is_empty(), "{src}: {w:?}");
            assert!(has_entry(&out, "Pictures/"), "{src}");
        }
    }

    #[test]
    fn budget_do_mdv_recusa_quando_o_contador_estoura() {
        // Não altera o limite global (os testes rodam em paralelo): só confere o contrato.
        assert!(MemBudget.check().is_ok());
        assert!(MemBudget.check_parse(10).is_ok());
        assert!(MemBudget.check_parse(usize::MAX).is_err(), "árvore gigante é barrada antes de analisar");
    }
}
