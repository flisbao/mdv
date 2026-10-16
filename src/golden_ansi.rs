//! Goldens de ANSI com cor: `render` + `to_ansi(.., true)` sobre documentos Markdown.
//! O ESC é gravado como `␛` para o diff ser legível. Os goldens ficam em `tests/golden/ansi/`.
//! Regenerar de propósito: `UPDATE_GOLDEN=1 cargo test golden_ansi`.

use crate::render::{render, to_ansi};
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
                Ok(want) if want == got => {}
                Ok(_) => bad.push(format!("{name}: a saída mudou")),
                Err(_) => bad.push(format!("{name}: golden ausente (rode com UPDATE_GOLDEN=1)")),
            }
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}
