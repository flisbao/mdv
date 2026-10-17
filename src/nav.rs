//! Navegação por links na TUI: decide o que um link faz (âncora, `.md` local, URL externa) e abre URLs.
//! Só decide e executa o mínimo; quem muda a tela é `main.rs`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Teto do tamanho de uma URL externa aceita para abrir (protege a linha de comando).
const MAX_URL_BYTES: usize = 2048;

/// O que fazer com o destino de um link.
#[derive(Debug, PartialEq, Eq)]
pub enum Target {
    /// `#âncora` no documento atual.
    Anchor(String),
    /// URL `http`, `https` ou `mailto`, para abrir fora (com confirmação).
    External(String),
    /// Arquivo Markdown relativo (confinado), com âncora opcional.
    Local { path: PathBuf, anchor: Option<String> },
    /// Não dá para seguir; o texto é o motivo.
    Unsupported(String),
}

/// Classifica `dest`. Caminhos locais são resolvidos a partir de `base` (pasta do documento atual).
pub fn resolve(dest: &str, base: Option<&Path>) -> Target {
    let dest = dest.trim();
    if let Some(a) = dest.strip_prefix('#') {
        return Target::Anchor(mdv_text::percent_decode(a));
    }
    if mdv_text::is_external(dest) {
        let scheme = dest.split(':').next().unwrap_or("").to_ascii_lowercase();
        let ok = matches!(scheme.as_str(), "http" | "https" | "mailto")
            && dest.len() <= MAX_URL_BYTES
            && !dest.chars().any(|c| c.is_control() || c.is_whitespace());
        // `mailto:` pode carregar `?attach=/caminho` (alguns clientes anexam o arquivo): só os
        // campos de texto da mensagem passam.
        let ok = ok && (scheme != "mailto" || mailto_params_ok(dest));
        return if ok {
            Target::External(dest.to_string())
        } else {
            Target::Unsupported(format!("esquema não suportado: {scheme}"))
        };
    }
    let (file, anchor) = match dest.split_once('#') {
        Some((f, a)) => (f, Some(mdv_text::percent_decode(a)).filter(|a| !a.is_empty())),
        None => (dest, None),
    };
    let file = mdv_text::percent_decode(file);
    if file.is_empty() || !mdv_text::is_confined(&file) {
        return Target::Unsupported("caminho fora da pasta do documento".to_string());
    }
    let ext = Path::new(&file).extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    if !matches!(ext.as_str(), "md" | "markdown") {
        return Target::Unsupported("só abro arquivos .md".to_string());
    }
    let Some(base) = base else {
        return Target::Unsupported("links relativos precisam de um arquivo (não stdin)".to_string());
    };
    Target::Local { path: base.join(file), anchor }
}

/// Os parâmetros da query de um `mailto:` são todos de texto (`subject`, `body`, `cc`, `bcc`, `to`)?
fn mailto_params_ok(url: &str) -> bool {
    let Some((_, query)) = url.split_once('?') else { return true };
    query.split('&').all(|kv| {
        let key = kv.split('=').next().unwrap_or("").to_ascii_lowercase();
        matches!(key.as_str(), "subject" | "body" | "cc" | "bcc" | "to")
    })
}

/// Índice do título cuja âncora (slug do GitHub, com `-1`, `-2` nos repetidos) é `anchor`.
pub fn heading_for_anchor(titles: &[&str], anchor: &str) -> Option<usize> {
    let want = mdv_text::slugify(anchor);
    let mut seen: HashMap<String, usize> = HashMap::new();
    for (i, t) in titles.iter().enumerate() {
        let base = mdv_text::slugify(t);
        let n = seen.entry(base.clone()).or_insert(0);
        let slug = if *n == 0 { base } else { format!("{base}-{n}") };
        *n += 1;
        if slug == want {
            return Some(i);
        }
    }
    None
}

/// Abre `url` no navegador do sistema, sem esperar nem herdar o terminal. `url` já passou por `resolve`.
pub fn open_external(url: &str) -> Result<(), String> {
    // Defesa em profundidade: o programa nunca pode tomar a URL por uma opção.
    if url.starts_with('-') {
        return Err("URL recusada".to_string());
    }
    let program = if cfg!(target_os = "macos") { "open" } else { "xdg-open" };
    let mut child = Command::new(program)
        .arg(url)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("não consegui executar {program}: {e}"))?;
    // Recolhe o processo em segundo plano para não deixar zumbi.
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ancora_e_externo() {
        assert_eq!(resolve("#Meu%20t", None), Target::Anchor("Meu t".into()));
        assert_eq!(resolve("https://a.b/c", None), Target::External("https://a.b/c".into()));
        assert!(matches!(resolve("javascript:alert(1)", None), Target::Unsupported(_)));
        assert!(matches!(resolve("file:///etc/passwd", None), Target::Unsupported(_)));
        assert!(matches!(resolve("https://a.b/ c", None), Target::Unsupported(_)));
        assert!(matches!(resolve("mailto:a@b.c?attach=/etc/passwd", None), Target::Unsupported(_)));
        assert!(matches!(resolve("mailto:a@b.c?subject=oi&body=x", None), Target::External(_)));
        assert!(open_external("--help").is_err());
    }

    #[test]
    fn local_confinado_so_md() {
        let base = Path::new("/d");
        assert_eq!(
            resolve("sub/a.doc.md#x", Some(base)),
            Target::Local { path: PathBuf::from("/d/sub/a.doc.md"), anchor: Some("x".into()) }
        );
        assert!(matches!(resolve("/etc/a.doc.md", Some(base)), Target::Unsupported(_)));
        assert!(matches!(resolve("%2Fetc%2Fa.md", Some(base)), Target::Unsupported(_)));
        assert!(matches!(resolve("../a.doc.md", Some(base)), Target::Unsupported(_)));
        assert!(matches!(resolve("%2e%2e/a.doc.md", Some(base)), Target::Unsupported(_)));
        assert!(matches!(resolve("sub/../../a.doc.md", Some(base)), Target::Unsupported(_)));
        assert!(matches!(resolve("a.png", Some(base)), Target::Unsupported(_)));
        assert!(matches!(resolve("a.doc.md", None), Target::Unsupported(_)));
    }

    #[test]
    fn ancora_com_repetidos() {
        let t = ["Intro", "Uso", "Uso"];
        assert_eq!(heading_for_anchor(&t, "uso"), Some(1));
        assert_eq!(heading_for_anchor(&t, "uso-1"), Some(2));
        assert_eq!(heading_for_anchor(&t, "nada"), None);
    }
}
