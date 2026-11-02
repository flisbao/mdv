//! Leitura da entrada: arquivo ou stdin, com teto de bytes, e o cache de imagens que a acompanha.

use std::fs;
use std::io::{self, IsTerminal, Read};
use std::path::{Path, PathBuf};

use crate::config::Config;
use crate::{cli, graphics, image, mem, net, Failure};

/// Teto de bytes lidos de arquivo ou stdin (protege a memória).
pub(crate) const MAX_INPUT_BYTES: u64 = 16 * 1024 * 1024;

/// Bytes em uma MiB, para exibir os tetos de tamanho nas mensagens.
const MIB: u64 = 1024 * 1024;

pub(crate) enum Source {
    File(PathBuf),
    Stdin,
}

/// O documento vem de stdin (`-` ou entrada redirecionada sem arquivo)?
pub(crate) fn reads_stdin(path: Option<&str>) -> bool {
    match path {
        Some(p) => p == "-",
        None => !io::stdin().is_terminal(),
    }
}

/// Lê o documento do arquivo informado, de stdin (`-` ou entrada redirecionada).
pub(crate) fn load_source(path: Option<&str>) -> Result<(Source, String), Failure> {
    match path {
        Some("-") => Ok((Source::Stdin, read_stdin().map_err(read_failure("stdin"))?)),
        Some(p) if Path::new(p).is_dir() => Err(Failure::Usage(format!(
            "{p} é uma pasta: abra sem -p num terminal para escolher o arquivo"
        ))),
        Some(p) => {
            let md = read_document(Path::new(p)).map_err(read_failure(p))?;
            Ok((Source::File(PathBuf::from(p)), md))
        }
        None if !io::stdin().is_terminal() => {
            Ok((Source::Stdin, read_stdin().map_err(read_failure("stdin"))?))
        }
        None => Err(Failure::Usage("nenhum arquivo informado".to_string())),
    }
}

/// Imagens relativas são resolvidas a partir da pasta do arquivo (ou do diretório atual, com stdin).
/// `--remote` baixa imagens de URLs; os diagramas Mermaid usam o renderizador nativo
/// (`blocking`: espera, como no `--plain`).
pub(crate) fn image_store(source: &Source, cli: &cli::Cli, blocking: bool) -> image::ImageStore {
    store_for(source, cli, blocking, &Config::load(), |k| std::env::var(k).ok())
}

/// `image_store` com a configuração e o ambiente injetados (testável).
fn store_for(
    source: &Source,
    cli: &cli::Cli,
    blocking: bool,
    config: &Config,
    env: impl Fn(&str) -> Option<String>,
) -> image::ImageStore {
    let base = match source {
        Source::File(p) => p
            .parent()
            .map(|d| if d.as_os_str().is_empty() { PathBuf::from(".") } else { d.to_path_buf() }),
        Source::Stdin => None,
    };
    // Gráficos só no modo interativo (`blocking == false`); `--plain` fica sempre em meio-bloco.
    let graphics = !blocking
        && match cli.common.graphics {
            cli::Graphics::Off => false,
            cli::Graphics::Kitty => true,
            cli::Graphics::Auto => graphics::supports_kitty(env),
        };
    let mut store = image::ImageStore::new(base).with_graphics(graphics);
    if cli.common.remote {
        store = store.with_remote(net::FetchPolicy::default(), blocking);
    }
    // Diagramas Mermaid saem do renderizador nativo, salvo `mermaid = off` na configuração.
    if config.mermaid {
        store = store.with_native_mermaid(blocking);
    }
    store
}

pub(crate) fn read_failure(what: &str) -> impl FnOnce(io::Error) -> Failure + '_ {
    move |e| Failure::Runtime(format!("não consegui ler {what}: {e}"))
}

pub(crate) fn read_stdin() -> io::Result<String> {
    read_limited(io::stdin().lock(), MAX_INPUT_BYTES)
}

/// Um formato que o `mdv` converte em Markdown: a extensão (sem ponto, minúscula) e o leitor.
pub(crate) struct Conversor {
    pub ext: &'static str,
    pub ler: fn(&Path) -> io::Result<String>,
}

/// O registro único dos formatos convertidos. Um formato novo entra só aqui: o seletor de pasta
/// (`picker::is_listed`), a recusa de `-o .md` (`cli`) e `e_convertido` o acompanham. Só a linha de
/// extensões do `HELP` é texto fixo, conferida por teste.
pub(crate) const CONVERSORES: &[Conversor] = &[
    Conversor { ext: "msg", ler: crate::msg::read_msg_file },
    Conversor { ext: "docx", ler: crate::docx::read_docx_file },
    Conversor { ext: "xlsx", ler: crate::xlsx::read_xlsx_file },
];

/// Leitor de um formato que o `mdv` converte em Markdown (ver `CONVERSORES`), ou `None`.
pub(crate) fn conversor(path: &Path) -> Option<fn(&Path) -> io::Result<String>> {
    let ext = path.extension().and_then(|e| e.to_str())?;
    CONVERSORES.iter().find(|c| ext.eq_ignore_ascii_case(c.ext)).map(|c| c.ler)
}

/// O arquivo é de um formato que o `mdv` converte em Markdown (tem `conversor`)?
pub(crate) fn e_convertido(path: &Path) -> bool {
    conversor(path).is_some()
}

/// As extensões convertidas para mensagens: `.msg, .docx e .xlsx`.
pub(crate) fn extensoes_convertidas() -> String {
    let nomes: Vec<String> = CONVERSORES.iter().map(|c| format!(".{}", c.ext)).collect();
    match nomes.split_last() {
        Some((ultimo, resto)) if !resto.is_empty() => format!("{} e {ultimo}", resto.join(", ")),
        _ => nomes.concat(),
    }
}

/// Lê o documento de um caminho: `.msg` vira Markdown pelo `mdv-msg`, `.docx` pelo `mdv-docx`, `.xlsx` pelo `mdv-xlsx` (ver `conversor`); planilha recusada dá erro; HTML vai para `html` e XML para `xml` (encoding declarado); o resto é Markdown puro.
pub(crate) fn read_document(path: &Path) -> io::Result<String> {
    if let Some(ler) = conversor(path) {
        ler(path)
    } else if let Some(recusa) = recusa(path) {
        Err(io::Error::new(io::ErrorKind::InvalidInput, recusa))
    } else if crate::html::is_html(path) {
        crate::html::read_html_file(path)
    } else if crate::xml::is_xml(path) {
        crate::xml::read_xml_file(path)
    } else {
        read_markdown_file(path)
    }
}

/// Formato de escritório que o `mdv` reconhece e recusa com uma mensagem clara (documento ou planilha).
fn recusa(path: &Path) -> Option<&'static str> {
    crate::docx::documento_nao_suportado(path).or_else(|| crate::xlsx::planilha_nao_suportada(path))
}

/// Lê um arquivo regular (nunca dispositivo, FIFO ou diretório) respeitando o teto.
pub(crate) fn read_markdown_file(path: &Path) -> io::Result<String> {
    let meta = fs::metadata(path)?;
    if !meta.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "não é um arquivo regular",
        ));
    }
    mem::check_input(meta.len())
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
    read_limited(fs::File::open(path)?, MAX_INPUT_BYTES)
}

/// Lê um arquivo binário regular (nunca dispositivo, FIFO ou diretório) de até `max` bytes.
/// Recusa antes de ler o que passa do teto (`arquivo {rotulo} grande demais`) ou do limite de memória.
pub(crate) fn read_binary_capped(path: &Path, max: u64, rotulo: &str) -> io::Result<Vec<u8>> {
    let meta = fs::metadata(path)?;
    if !meta.is_file() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "não é um arquivo regular"));
    }
    if meta.len() > max {
        let msg = format!("arquivo {rotulo} grande demais (máximo {} MiB)", max / MIB);
        return Err(io::Error::new(io::ErrorKind::InvalidData, msg));
    }
    mem::check_input(meta.len()).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
    read_limited_bytes(fs::File::open(path)?, max)
}

/// Lê até `max` bytes de `reader` (sem ler além de `max + 1`), falhando se passar do teto.
pub(crate) fn read_limited_bytes(reader: impl Read, max: u64) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader.take(max + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > max {
        let msg = format!("entrada maior que {} MiB", max / MIB);
        return Err(io::Error::new(io::ErrorKind::InvalidData, msg));
    }
    Ok(bytes)
}

/// Lê UTF-8 de `reader`, falhando se passar de `max` bytes.
pub(crate) fn read_limited(reader: impl Read, max: u64) -> io::Result<String> {
    let bytes = read_limited_bytes(reader, max)?;
    String::from_utf8(bytes).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    #[test]
    fn read_document_converte_docx_msg_e_devolve_md_intacto() {
        let d = tempfile::tempdir().unwrap();
        let msg = d.path().join("oi.msg");
        fs::write(&msg, mdv_msg::testutil::MsgBuilder::new().string(mdv_msg::testutil::tags::SUBJECT, "Oi").build())
            .unwrap();
        let texto = read_document(&msg).unwrap();
        assert!(texto.contains("# Oi"), "{texto}");
        let docx = d.path().join("oi.docx");
        let corpo = "<w:p><w:r><w:t>Olá docx</w:t></w:r></w:p>";
        fs::write(&docx, mdv_docx::testutil::DocxBuilder::novo().corpo(corpo).construir()).unwrap();
        assert!(read_document(&docx).unwrap().contains("Olá docx"));
        let xlsx = d.path().join("planilha.xlsx");
        let linha = r#"<row r="1"><c r="A1" t="inlineStr"><is><t>Olá planilha</t></is></c></row>"#;
        fs::write(&xlsx, mdv_xlsx::testutil::XlsxBuilder::novo().aba("P", linha).construir()).unwrap();
        assert!(read_document(&xlsx).unwrap().contains("Olá planilha"));
        let md = d.path().join("nota.md");
        fs::write(&md, "# Nota\n\ntexto *puro*\n").unwrap();
        assert_eq!(read_document(&md).unwrap(), "# Nota\n\ntexto *puro*\n");
    }

    #[test]
    fn read_document_decodifica_xml_pelo_encoding_declarado() {
        let d = tempfile::tempdir().unwrap();
        let xml = d.path().join("a.xml");
        fs::write(&xml, b"<?xml version=\"1.0\" encoding=\"ISO-8859-1\"?><a>caf\xe9</a>").unwrap();
        assert!(read_document(&xml).unwrap().contains("café"));
    }

    #[test]
    fn conversor_so_existe_para_msg_docx_e_xlsx() {
        for nome in ["a.msg", "a.DOCX", "a.xlsx"] {
            let p = Path::new(nome);
            assert!(conversor(p).is_some() && e_convertido(p), "{nome}");
        }
        for nome in ["a.md", "a.html", "a.xls", "a.ods", "a", "docx", "a.xlsx.md"] {
            let p = Path::new(nome);
            assert!(conversor(p).is_none() && !e_convertido(p), "{nome}");
        }
    }

    #[test]
    fn extensoes_convertidas_listam_o_registro() {
        assert_eq!(extensoes_convertidas(), ".msg, .docx e .xlsx");
    }

    #[test]
    fn ajuda_cita_todos_os_formatos_convertidos() {
        let linha = crate::plain::HELP.lines().find(|l| l.contains("deduzido da extensão")).expect("linha de extensões");
        for c in CONVERSORES {
            assert!(linha.contains(&format!(".{}", c.ext)), "a linha de extensões do HELP não cita .{}", c.ext);
        }
    }

    fn kitty(k: &str) -> Option<String> {
        (k == "TERM").then(|| "xterm-kitty".to_string())
    }

    fn mermaid_on(args: &[&str], blocking: bool, cfg: &Config, env: fn(&str) -> Option<String>) -> bool {
        let cli = cli::parse(args.iter().map(|a| (*a).to_string())).unwrap();
        store_for(&Source::Stdin, &cli, blocking, cfg, env).mermaid_enabled()
    }

    #[test]
    fn mermaid_liga_sozinho_em_terminal_kitty() {
        let on = Config::default();
        assert_eq!(mermaid_on(&["a.md"], false, &on, kitty), cfg!(feature = "mermaid"));
        assert!(!mermaid_on(&["a.md"], false, &on, |_| None), "terminal sem suporte: só código");
        assert!(!mermaid_on(&["a.md"], true, &on, kitty), "plain nunca desenha");
        assert!(!mermaid_on(&["--graphics", "off", "a.md"], false, &on, kitty));
    }

    #[test]
    fn config_desligada_nao_desenha_mermaid_nem_com_kitty() {
        let off = Config { mermaid: false };
        assert!(!mermaid_on(&["a.md"], false, &off, kitty));
        assert!(!mermaid_on(&["--graphics", "kitty", "a.md"], false, &off, kitty));
    }

    #[test]
    fn read_limited_aceita_exatamente_o_teto() {
        let text = read_limited(&b"abcd"[..], 4).unwrap();
        assert_eq!(text, "abcd");
    }

    #[test]
    fn read_limited_rejeita_acima_do_teto() {
        let err = read_limited(&b"abcde"[..], 4).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn read_limited_nao_le_alem_do_teto() {
        let mut infinite = io::repeat(b'a');
        assert!(read_limited(&mut infinite, 1024).is_err());
    }

    #[test]
    fn read_limited_rejeita_utf8_invalido() {
        let err = read_limited(&[0xff, 0xfe][..], 16).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn read_markdown_file_rejeita_diretorio() {
        let err = read_markdown_file(Path::new(".")).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
    }

    #[cfg(unix)]
    #[test]
    fn read_markdown_file_rejeita_dispositivo() {
        let err = read_markdown_file(Path::new("/dev/zero")).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
    }
}
