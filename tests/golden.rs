//! Goldens de texto: executa o binário em modo `--plain` sobre `samples/` e compara a saída
//! (sem cor, porque o stdout do teste não é um terminal) com `tests/golden/*.txt`.
//! Regenerar de propósito: `UPDATE_GOLDEN=1 cargo test --test golden`.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

/// Documentos exibidos com `-p -w <largura> <arquivo>`.
const FILES: &[&str] = &[
    "sample.md",
    "samples/basico.md",
    "samples/codigo.md",
    "samples/longo.md",
    "samples/tabelas.md",
    "samples/unicode.md",
    "samples/img.md",
    "samples/log.txt",
    "tests/fixtures/gfm.md",
    "samples/dados.csv",
    "samples/dados.tsv",
    "samples/vendas.csv",
    "samples/dados.json",
    "samples/itens.json",
    "samples/eventos.jsonl",
];

const WIDTHS: [usize; 2] = [40, 80];

/// Saídas de texto que não passam por `-w`: (nome do golden, argumentos).
const EXTRAS: &[(&str, &[&str])] = &[
    ("toc.longo", &["--toc", "samples/longo.md"]),
    ("toc.gfm", &["--toc", "tests/fixtures/gfm.md"]),
    ("stats.longo", &["--stats", "samples/longo.md"]),
    ("stats.gfm", &["--stats", "tests/fixtures/gfm.md"]),
    ("json-schema.itens", &["json", "-p", "-w", "80", "samples/itens.json", "--as", "schema"]),
    ("json-table.itens", &["json", "-p", "-w", "80", "samples/itens.json", "--as", "table"]),
];

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn run(args: &[&str]) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_mdv"))
        .args(args)
        .current_dir(root())
        .env_remove("MDV_MEMORY_BUFFER")
        .output()
        .expect("mdv deve iniciar");
    assert!(out.status.success(), "{args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8(out.stdout).expect("saída em UTF-8")
}

fn compare(name: &str, got: &str, bad: &mut Vec<String>) {
    let path = root().join("tests/golden").join(format!("{name}.txt"));
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, got).unwrap();
        return;
    }
    match fs::read_to_string(&path) {
        Ok(want) if want.replace("\r\n", "\n") == got => {}
        Ok(_) => bad.push(format!("{name}: a saída mudou")),
        Err(_) => bad.push(format!("{name}: golden ausente (rode com UPDATE_GOLDEN=1)")),
    }
}

#[test]
fn golden_texto_nao_muda() {
    let mut bad = Vec::new();
    for file in FILES {
        for w in WIDTHS {
            let got = run(&["-p", "-w", &w.to_string(), file]);
            compare(&format!("{}.w{w}", file.replace('/', "__")), &got, &mut bad);
        }
    }
    for (name, args) in EXTRAS {
        compare(name, &run(args), &mut bad);
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}
