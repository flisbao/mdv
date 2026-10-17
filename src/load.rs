//! Leitura da entrada: arquivo ou stdin, com teto de bytes, e o cache de imagens que a acompanha.

use std::fs;
use std::io::{self, IsTerminal, Read};
use std::path::{Path, PathBuf};

use crate::{cli, graphics, image, mem, net, Failure};

/// Teto de bytes lidos de arquivo ou stdin (protege a memória).
pub(crate) const MAX_INPUT_BYTES: u64 = 16 * 1024 * 1024;

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
            let md = read_markdown_file(Path::new(p)).map_err(read_failure(p))?;
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
    let base = match source {
        Source::File(p) => p
            .parent()
            .map(|d| if d.as_os_str().is_empty() { PathBuf::from(".") } else { d.to_path_buf() }),
        Source::Stdin => None,
    };
    // Gráficos só no modo interativo (`blocking == false`); `--plain` fica sempre em meio-bloco.
    let graphics = !blocking
        && match cli.graphics {
            cli::Graphics::Off => false,
            cli::Graphics::Kitty => true,
            cli::Graphics::Auto => graphics::supports_kitty(|k| std::env::var(k).ok()),
        };
    let mut store = image::ImageStore::new(base).with_graphics(graphics);
    if cli.remote {
        store = store.with_remote(net::FetchPolicy::default(), blocking);
    }
    // Diagramas Mermaid saem do renderizador nativo.
    store = store.with_native_mermaid(blocking);
    store
}

pub(crate) fn read_failure(what: &str) -> impl FnOnce(io::Error) -> Failure + '_ {
    move |e| Failure::Runtime(format!("não consegui ler {what}: {e}"))
}

pub(crate) fn read_stdin() -> io::Result<String> {
    read_limited(io::stdin().lock(), MAX_INPUT_BYTES)
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

/// Lê UTF-8 de `reader`, falhando se passar de `max` bytes (sem ler além de `max + 1`).
pub(crate) fn read_limited(reader: impl Read, max: u64) -> io::Result<String> {
    let mut bytes = Vec::new();
    reader.take(max + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > max {
        let msg = format!("entrada maior que {} MiB", max / (1024 * 1024));
        return Err(io::Error::new(io::ErrorKind::InvalidData, msg));
    }
    String::from_utf8(bytes).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

#[cfg(test)]
mod tests {
    use super::*;

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
