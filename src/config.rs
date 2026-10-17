//! Arquivo de configuração do usuário (`$XDG_CONFIG_HOME/mdv/config`, `%APPDATA%/mdv/config` ou `~/.config/mdv/config`).
//!
//! Formato: linhas `chave = valor`, `#` começa comentário. Chave conhecida: `mermaid` (`on`/`off`).
//! Chave ou valor desconhecido é ignorado, para um arquivo antigo ou errado nunca impedir a leitura.

use std::io::Read;
use std::path::PathBuf;

/// Teto do arquivo de configuração: ele só guarda um punhado de linhas.
const MAX_CONFIG_BYTES: u64 = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// Desenhar os blocos ```mermaid como imagem quando o terminal suporta (padrão: sim).
    pub mermaid: bool,
}

impl Default for Config {
    fn default() -> Self {
        Config { mermaid: true }
    }
}

impl Config {
    pub fn parse(text: &str) -> Config {
        let mut cfg = Config::default();
        for line in text.lines() {
            let line = line.split('#').next().unwrap_or("");
            let Some((key, value)) = line.split_once('=') else { continue };
            if key.trim() == "mermaid" {
                match value.trim().to_ascii_lowercase().as_str() {
                    "on" | "true" | "yes" | "1" => cfg.mermaid = true,
                    "off" | "false" | "no" | "0" => cfg.mermaid = false,
                    _ => {}
                }
            }
        }
        cfg
    }

    /// Lê o arquivo do usuário; ausente, ilegível ou grande demais vale o padrão.
    pub fn load() -> Config {
        let Some(path) = path_from(|k| std::env::var(k).ok()) else { return Config::default() };
        let Ok(file) = std::fs::File::open(path) else { return Config::default() };
        let mut buf = Vec::new();
        if file.take(MAX_CONFIG_BYTES + 1).read_to_end(&mut buf).is_err() || buf.len() as u64 > MAX_CONFIG_BYTES {
            return Config::default();
        }
        Config::parse(&String::from_utf8_lossy(&buf))
    }
}

fn path_from(var: impl Fn(&str) -> Option<String>) -> Option<PathBuf> {
    let set = |k: &str| var(k).filter(|v| !v.is_empty());
    let base = match set("XDG_CONFIG_HOME").or_else(|| set("APPDATA")) {
        Some(x) => PathBuf::from(x),
        None => PathBuf::from(var("HOME").filter(|v| !v.is_empty())?).join(".config"),
    };
    Some(base.join("mdv").join("config"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn padrao_liga_o_mermaid() {
        assert!(Config::default().mermaid);
        assert!(Config::parse("").mermaid);
    }

    #[test]
    fn mermaid_off_desliga() {
        for v in ["off", "false", "no", "0", "OFF"] {
            assert!(!Config::parse(&format!("mermaid = {v}")).mermaid, "{v}");
        }
        assert!(Config::parse("mermaid=on").mermaid);
    }

    #[test]
    fn comentarios_e_lixo_sao_ignorados() {
        let c = Config::parse("# mermaid = off\nfoo = bar\nmermaid = talvez\n\nsem igual\n");
        assert!(c.mermaid);
        assert!(!Config::parse("# x\nmermaid = off # desligado\n").mermaid);
    }

    #[test]
    fn caminho_usa_xdg_e_depois_home() {
        let env = |k: &str| match k {
            "XDG_CONFIG_HOME" => Some("/x".to_string()),
            "HOME" => Some("/h".to_string()),
            _ => None,
        };
        assert_eq!(path_from(env).unwrap(), PathBuf::from("/x").join("mdv").join("config"));
        let env = |k: &str| (k == "HOME").then(|| "/h".to_string());
        assert_eq!(path_from(env).unwrap(), PathBuf::from("/h").join(".config").join("mdv").join("config"));
        // Windows: `%APPDATA%` vem antes de `HOME`.
        let env = |k: &str| match k {
            "APPDATA" => Some("/a".to_string()),
            "HOME" => Some("/h".to_string()),
            _ => None,
        };
        assert_eq!(path_from(env).unwrap(), PathBuf::from("/a").join("mdv").join("config"));
        assert!(path_from(|_| None).is_none());
    }
}
