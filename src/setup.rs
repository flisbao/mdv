//! `mdv --setup-mermaid`: diagnostica o ambiente para os diagramas Mermaid e mostra o que fazer.
//! Só lê e informa: nunca instala nem baixa nada.

use std::ffi::OsStr;
use std::io::Read;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::render::sanitize;

/// Tempo máximo para `node --version` / `mmdc --version`.
const PROBE_TIMEOUT: Duration = Duration::from_secs(5);

/// O que foi encontrado na máquina.
pub struct Findings {
    pub os: &'static str,
    pub fonts: usize,
    pub node: Option<String>,
    pub mmdc: Option<String>,
    pub browser: Option<String>,
    /// O terminal atual mostra alta resolução (kitty/Ghostty, fora do `tmux`/`screen`)?
    pub kitty: bool,
}

/// Primeira linha, sem caracteres de controle.
fn first_line(s: &str) -> String {
    sanitize(s.lines().next().unwrap_or("").trim()).into_owned()
}

/// Roda `bin arg` com limite de tempo e devolve a primeira linha de stdout.
fn probe_version(bin: &str, arg: &str) -> Option<String> {
    let mut child = Command::new(bin)
        .arg(arg)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut stdout = child.stdout.take()?;
    // Lido numa thread: um filho com muita saída não trava, e um neto que herde o stdout
    // não prende a leitura sem prazo.
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = (&mut stdout).take(1024).read_to_end(&mut buf);
        let _ = tx.send(buf);
        // Descarta o resto: fechar o pipe cedo mataria o filho com SIGPIPE e a sonda falharia.
        let _ = std::io::copy(&mut stdout, &mut std::io::sink());
    });
    let start = Instant::now();
    let status = loop {
        match child.try_wait().ok()? {
            Some(s) => break s,
            None if start.elapsed() > PROBE_TIMEOUT => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
            None => std::thread::sleep(Duration::from_millis(25)),
        }
    };
    if !status.success() {
        return None;
    }
    let buf = rx.recv_timeout(Duration::from_millis(500)).ok()?;
    let line = first_line(&String::from_utf8_lossy(&buf));
    (!line.is_empty()).then_some(line)
}

/// `name` existe em algum diretório do `path` dado? (no Windows, também com `.exe`)
fn on_path(name: &str, path: Option<&OsStr>) -> bool {
    let Some(path) = path else { return false };
    std::env::split_paths(path).any(|dir| dir.join(name).is_file() || dir.join(format!("{name}.exe")).is_file())
}

/// Chromium/Chrome utilizável pelo `mmdc`: variável do puppeteer, binário no PATH ou cache do puppeteer.
fn find_browser() -> Option<String> {
    if let Ok(p) = std::env::var("PUPPETEER_EXECUTABLE_PATH") {
        if std::path::Path::new(&p).is_file() {
            return Some(first_line(&p));
        }
    }
    let path = std::env::var_os("PATH");
    for name in ["google-chrome", "google-chrome-stable", "chromium", "chromium-browser", "chrome", "msedge"] {
        if on_path(name, path.as_deref()) {
            return Some(name.to_string());
        }
    }
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"))?;
    let cache = std::path::Path::new(&home).join(".cache").join("puppeteer");
    cache.is_dir().then(|| "cache do puppeteer".to_string())
}

pub fn gather() -> Findings {
    let mmdc_bin = std::env::var("MDV_MMDC").unwrap_or_else(|_| "mmdc".to_string());
    Findings {
        os: std::env::consts::OS,
        fonts: crate::mermaid_native::font_count(),
        node: probe_version("node", "--version"),
        mmdc: probe_version(&mmdc_bin, "--version"),
        browser: find_browser(),
        kitty: crate::graphics::supports_kitty(|k| std::env::var(k).ok()),
    }
}

fn mark(ok: bool) -> &'static str {
    if ok {
        "✔"
    } else {
        "✘"
    }
}

fn install_hint(os: &str) -> &'static str {
    match os {
        "macos" => "brew install node && npm install -g @mermaid-js/mermaid-cli",
        "windows" => "instale o Node.js (https://nodejs.org) e rode: npm install -g @mermaid-js/mermaid-cli",
        "linux" => {
            "instale o Node.js pelo gerenciador de pacotes da distribuição (ou https://nodejs.org) e rode: npm install -g @mermaid-js/mermaid-cli"
        }
        _ => "instale o Node.js (https://nodejs.org) e rode: npm install -g @mermaid-js/mermaid-cli",
    }
}

pub fn format(f: &Findings) -> String {
    let mut o = String::from("mdv --setup-mermaid\n\n");

    o.push_str("Renderizador nativo (embutido)\n");
    if f.fonts > 0 {
        o.push_str(&format!(
            "  ✔ disponível, {} fontes do sistema encontradas: não precisa instalar nada.\n",
            f.fonts
        ));
        o.push_str("  Desenha flowchart, sequence, class, state, ER, pie, Gantt, timeline, journey, mindmap e outros.\n");
        o.push_str("  Diagramas muito complexos podem sair diferentes do mermaid.js; com --mermaid o mdv tenta o mmdc nesses casos.\n");
    } else {
        o.push_str("  ✘ nenhuma fonte encontrada: sem fontes o texto dos diagramas não pode ser desenhado.\n");
        o.push_str("  Instale ao menos uma fonte (por exemplo, DejaVu ou Noto) ou use o mmdc abaixo (com --mermaid).\n");
    }

    o.push_str("\nmmdc (opcional, para o que o nativo não desenha)\n");
    match &f.node {
        Some(v) => o.push_str(&format!("  ✔ Node.js: {v}\n")),
        None => o.push_str("  ✘ Node.js não encontrado\n"),
    }
    match &f.mmdc {
        Some(v) => o.push_str(&format!("  ✔ mmdc: {v}\n")),
        None => o.push_str("  ✘ mmdc não encontrado (ou defina MDV_MMDC com o caminho)\n"),
    }
    match &f.browser {
        Some(b) => o.push_str(&format!("  {} Chromium/Chrome: {b}\n", mark(true))),
        None => o.push_str(&format!("  {} Chromium/Chrome não encontrado (o mmdc o baixa ao instalar)\n", mark(false))),
    }
    if f.mmdc.is_none() {
        o.push_str(&format!("  Para instalar: {}\n", install_hint(f.os)));
        o.push_str("  A instalação baixa um Chromium (centenas de MB). O mdv não instala nada sozinho.\n");
    }
    o.push_str("  O mmdc abre um Chromium headless: use --mermaid só com documentos em que você confia.\n");

    o.push_str("\nTerminal\n");
    if f.kitty {
        o.push_str("  ✔ Terminal com alta resolução: --graphics auto desenha os diagramas como imagem.\n");
    } else {
        o.push_str("  Este terminal usará meio-bloco (funciona em qualquer terminal com cores 24 bits).\n");
    }
    o.push_str("  Alta resolução (--graphics kitty|auto): confirmada no kitty e no Ghostty (Linux e macOS).\n");
    o.push_str("  WezTerm, iTerm2, Warp e Konsole implementam o protocolo Kitty, mas o suporte a placeholders\n");
    o.push_str("  Unicode (usado pelo mdv) não foi confirmado: use --graphics kitty por sua conta.\n");
    o.push_str("  Windows Terminal não suporta o protocolo Kitty: fica no meio-bloco.\n");
    o
}

/// Coleta os achados e monta o relatório.
pub fn report() -> String {
    format(&gather())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn achados() -> Findings {
        Findings {
            os: "linux",
            fonts: 12,
            node: Some("v20.11.0".into()),
            mmdc: None,
            browser: None,
            kitty: false,
        }
    }

    #[test]
    fn relatorio_diz_que_o_nativo_funciona_sem_instalar_nada() {
        let t = format(&achados());
        assert!(t.contains("Renderizador nativo") && t.contains("12 fontes"), "{t}");
        assert!(t.contains("não precisa instalar nada"), "{t}");
    }

    #[test]
    fn sem_fontes_o_relatorio_avisa() {
        let mut a = achados();
        a.fonts = 0;
        let t = format(&a);
        assert!(t.contains("nenhuma fonte"), "{t}");
    }

    #[test]
    fn marca_o_que_existe_e_o_que_falta_do_mmdc() {
        let t = format(&achados());
        assert!(t.contains("✔ Node.js: v20.11.0"), "{t}");
        assert!(t.contains("✘ mmdc"), "{t}");
        assert!(t.contains("✘ Chromium"), "{t}");
        assert!(t.contains("npm install -g @mermaid-js/mermaid-cli"), "{t}");
    }

    #[test]
    fn instrucao_de_instalacao_depende_do_sistema() {
        let mut a = achados();
        a.os = "macos";
        assert!(format(&a).contains("brew install node"));
        a.os = "windows";
        assert!(format(&a).contains("https://nodejs.org"));
        a.os = "linux";
        assert!(format(&a).contains("gerenciador de pacotes"));
    }

    #[test]
    fn mmdc_completo_nao_mostra_instrucao_de_instalacao() {
        let mut a = achados();
        a.mmdc = Some("10.9.0".into());
        a.browser = Some("/usr/bin/chromium".into());
        let t = format(&a);
        assert!(t.contains("✔ mmdc: 10.9.0") && t.contains("✔ Chromium"), "{t}");
        assert!(!t.contains("npm install"), "{t}");
    }

    #[test]
    fn secao_do_terminal_lista_os_confirmados_e_o_que_nao_suporta() {
        let t = format(&achados());
        assert!(t.contains("kitty") && t.contains("Ghostty"), "{t}");
        assert!(t.contains("Windows Terminal"), "{t}");
        assert!(t.contains("meio-bloco"), "{t}");
        let mut a = achados();
        a.kitty = true;
        assert!(format(&a).contains("✔ Terminal com alta resolução"));
    }

    #[test]
    fn sem_fonte_sanitizada_a_versao_do_node_nao_traz_controles() {
        assert_eq!(first_line("v20.1\x1b[31m\nresto"), "v20.1\u{FFFD}[31m");
    }

    #[test]
    fn procura_binario_no_path_injetado() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("chromium"), "").unwrap();
        let path = std::env::join_paths([dir.path()]).unwrap();
        assert!(on_path("chromium", Some(&path)));
        assert!(!on_path("nao-existe-xyz", Some(&path)));
        assert!(!on_path("chromium", None));
    }

    #[test]
    fn versao_de_binario_inexistente_e_none_e_nao_trava() {
        let t = std::time::Instant::now();
        assert!(probe_version("/nao/existe/node", "--version").is_none());
        assert!(t.elapsed() < std::time::Duration::from_secs(2));
    }

    #[cfg(unix)]
    #[test]
    fn versao_longa_cortada_no_meio_de_caractere_nao_vira_none() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("longo");
        // 1201 bytes numa linha; o corte em 1024 cai no meio de um 'é' (2 bytes em UTF-8).
        std::fs::write(&p, "#!/bin/sh\nprintf 'a'\nprintf 'é%.0s' $(seq 1 600)\n").unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(probe_version(p.to_str().unwrap(), "--version").is_some());
    }

    #[cfg(unix)]
    #[test]
    fn versao_que_demora_demais_e_cortada() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("lento");
        std::fs::write(&p, "#!/bin/sh\nsleep 10\n").unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        let t = std::time::Instant::now();
        assert!(probe_version(p.to_str().unwrap(), "--version").is_none());
        assert!(t.elapsed() < std::time::Duration::from_secs(8));
    }

    #[cfg(unix)]
    #[test]
    fn sonda_com_saida_maior_que_o_teto_ainda_acha_a_versao() {
        use std::os::unix::fs::PermissionsExt;
        let d = tempfile::tempdir().unwrap();
        let bin = d.path().join("falso-bin");
        // ~5,5 KB no stdout (o teto lido é 1 KiB): antes, o pipe fechado matava o script.
        let script = "#!/bin/sh\ni=0\nwhile [ $i -lt 500 ]; do echo 0123456789; i=$((i+1)); done\n";
        std::fs::write(&bin, script).unwrap();
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        let v = probe_version(bin.to_str().unwrap(), "--version");
        assert_eq!(v.as_deref(), Some("0123456789"));
    }
}
