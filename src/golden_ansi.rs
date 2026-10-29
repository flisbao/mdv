//! Goldens de ANSI com cor: `render` + `to_ansi(.., true)` sobre documentos Markdown.
//! O ESC é gravado como `␛` para o diff ser legível. Os goldens ficam em `tests/golden/ansi/`.
//! Regenerar de propósito: `UPDATE_GOLDEN=1 cargo test golden_ansi`.

use crate::cli::{JsonView, DEFAULT_HEAD};
use crate::data::{render_json, JsonOpts};
use crate::render::{render, to_ansi};
use crate::yaml_view::render_yaml;
use std::fs;
use std::path::PathBuf;

const FILES: &[&str] = &[
    "sample.md",
    "samples/basico.md",
    "samples/codigo.md",
    "samples/longo.md",
    "samples/tabelas.md",
    "samples/unicode.md",
    "tests/fixtures/gfm.md",
];

const WIDTHS: [usize; 2] = [40, 80];

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

#[test]
fn ansi_com_cor_nao_muda() {
    let update = std::env::var_os("UPDATE_GOLDEN").is_some();
    let mut bad = Vec::new();
    for file in FILES {
        let md = fs::read_to_string(root().join(file)).unwrap_or_else(|e| panic!("{file}: {e}"));
        for w in WIDTHS {
            let got = to_ansi(&render(&md, w).lines, true).replace('\x1b', "␛");
            let name = format!("{}.w{w}.ansi", file.replace('/', "__"));
            let path = root().join("tests/golden/ansi").join(&name);
            if update {
                fs::create_dir_all(path.parent().unwrap()).unwrap();
                fs::write(&path, &got).unwrap();
                continue;
            }
            match fs::read_to_string(&path) {
                Ok(want) if want.replace("\r\n", "\n") == got => {}
                Ok(_) => bad.push(format!("{name}: a saída mudou")),
                Err(_) => bad.push(format!("{name}: golden ausente (rode com UPDATE_GOLDEN=1)")),
            }
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

/// YAML e JSON de `samples/` (válidos e quebrados) pela vista automática, com cor.
const DADOS: &[&str] = &["samples/config.yaml", "samples/quebrado.yaml", "samples/quebrado.json"];

#[test]
fn ansi_de_dados_com_cor_nao_muda() {
    let update = std::env::var_os("UPDATE_GOLDEN").is_some();
    let mut bad = Vec::new();
    for file in DADOS {
        let text = fs::read_to_string(root().join(file)).unwrap_or_else(|e| panic!("{file}: {e}"));
        let opts = JsonOpts { path: None, view: JsonView::Auto, head: DEFAULT_HEAD, cols: Vec::new() };
        for w in WIDTHS {
            let rendered = if file.ends_with(".json") { render_json(&text, &opts, w) } else { render_yaml(&text, &opts, w) };
            let got = to_ansi(&rendered.lines, true).replace('\x1b', "␛");
            let name = format!("{}.w{w}.ansi", file.replace('/', "__"));
            let path = root().join("tests/golden/ansi").join(&name);
            if update {
                fs::write(&path, &got).unwrap();
                continue;
            }
            match fs::read_to_string(&path) {
                Ok(want) if want.replace("\r\n", "\n") == got => {}
                Ok(_) => bad.push(format!("{name}: a saída mudou")),
                Err(_) => bad.push(format!("{name}: golden ausente (rode com UPDATE_GOLDEN=1)")),
            }
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}
