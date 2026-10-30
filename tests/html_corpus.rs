//! Régua de aceitação do corpus HTML (`samples/html/`): para cada amostra e cada largura em
//! [60, 80, 120], executa `mdv -p -w N <arquivo>` e confere os critérios 1 a 5 da definição de
//! "renderização legal" (docs/superpowers/specs/2026-10-30-leitura-html-design.md). Falhas não
//! param a coleta: todas viram uma tabela amostra × critério, impressa por `relatorio_do_corpus`.
//! O critério 6 (golden) é `golden_html_nao_muda`: largura 80, sem cor, em tests/golden/html/.
//! Regenerar goldens de propósito: `UPDATE_GOLDEN=1 cargo test --test html_corpus`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use mdv_html::color::contrast;
use mdv_html::{Options, Unlimited};
use unicode_width::UnicodeWidthStr;

const AMOSTRAS: [&str; 16] = [
    "artigo-blog",
    "changelog",
    "curriculo",
    "dashboard",
    "docs-api",
    "formulario",
    "graficos-css",
    "hostil",
    "landing",
    "legado-html4",
    "log-ansi",
    "newsletter-email",
    "relatorio-financeiro",
    "slides-claro",
    "slides-produto",
    "slides-secoes",
];

const LARGURAS: [usize; 3] = [60, 80, 120];

/// Largura do golden (critério 6).
const GOLDEN_LARGURA: usize = 80;

/// Amostras que mostram tags dentro de `<pre>`: nelas só os termos de CSS/JS são proibidos.
const PODE_MOSTRAR_TAGS: [&str; 2] = ["docs-api", "log-ansi"];

/// Trechos que não podem aparecer no texto saído (critério 3).
const PROIBIDOS: [&str; 9] = [
    "function(",
    "@import",
    "@font-face",
    "{color",
    "<div",
    "<span",
    "<script",
    "</",
    "style=",
];

/// Os trechos de marcação de `PROIBIDOS` que `PODE_MOSTRAR_TAGS` dispensa.
const PROIBIDOS_TAGS: [&str; 3] = ["<div", "<span", "</"];

/// Razão de contraste mínima (critério 5).
const CONTRASTE_MINIMO: f32 = 2.0;

const SUCESSO: usize = 0;
const LARGURA: usize = 1;
const VAZAMENTO: usize = 2;
const TEXTO: usize = 3;
const CONTRASTE: usize = 4;

const CRITERIOS: [&str; 5] = ["1 sucesso", "2 largura", "3 vazamento", "4 texto", "5 contraste"];

/// Uma falha de uma amostra num critério (as mensagens de todas as larguras vêm juntas).
struct Falha {
    amostra: &'static str,
    criterio: usize,
    mensagem: String,
}

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Roda `mdv -p -w <w> <arquivo>` (saída sem cor, porque o stdout é um pipe).
fn rode(arquivo: &Path, w: usize) -> Result<String, String> {
    let out = Command::new(env!("CARGO_BIN_EXE_mdv"))
        .args(["-p", "-w", &w.to_string()])
        .arg(arquivo)
        .current_dir(root())
        .env_remove("MDV_MEMORY_BUFFER")
        .output()
        .map_err(|e| format!("não iniciou: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "saiu com {:?}: {}",
            out.status.code(),
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    String::from_utf8(out.stdout).map_err(|_| "saída não é UTF-8".to_string())
}

/// Remove sequências ANSI CSI (`ESC [ … letra`).
fn sem_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            let mut resto = chars.clone();
            if resto.next() == Some('[') {
                chars = resto;
                for d in chars.by_ref() {
                    if d.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
            continue;
        }
        out.push(c);
    }
    out
}

/// Espaços e quebras de linha colapsados (a saída quebra as linhas na largura pedida). As bordas
/// de caixa (`▌`, `▐`, `│`, `┃`, `║`) que cercam cada linha de um bloco quebrado também saem, já
/// que não fazem parte do texto.
fn norm(s: &str) -> String {
    const BORDAS: &[char] = &['▌', '▐', '│', '┃', '║'];
    s.split_whitespace()
        .map(|p| p.trim_matches(BORDAS))
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Entidades mais comuns, para o texto de `data-esperado` ser comparado como aparece na tela.
fn decodifica(s: &str) -> String {
    s.replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&amp;", "&")
}

/// Todo valor de `data-esperado="…"` do fonte, por varredura simples.
fn textos_esperados(fonte: &str) -> Vec<String> {
    const CHAVE: &str = "data-esperado=\"";
    let mut textos = Vec::new();
    let mut resto = fonte;
    while let Some(pos) = resto.find(CHAVE) {
        let depois = &resto[pos + CHAVE.len()..];
        let Some(fim) = depois.find('"') else { break };
        textos.push(decodifica(&depois[..fim]));
        resto = &depois[fim + 1..];
    }
    textos
}

/// Avalia uma amostra nas três larguras e devolve uma falha por critério que falhou.
fn avalia(nome: &'static str) -> Vec<Falha> {
    let arquivo = root().join("samples/html").join(format!("{nome}.html"));
    let fonte = match fs::read_to_string(&arquivo) {
        Ok(t) => t,
        Err(e) => {
            return vec![Falha { amostra: nome, criterio: SUCESSO, mensagem: format!("amostra ilegível: {e}") }];
        }
    };
    let esperados = textos_esperados(&fonte);
    let proibidos: Vec<&str> = PROIBIDOS
        .iter()
        .copied()
        .filter(|p| !(PODE_MOSTRAR_TAGS.contains(&nome) && PROIBIDOS_TAGS.contains(p)))
        .collect();
    let mut por_criterio: Vec<Vec<String>> = vec![Vec::new(); CRITERIOS.len()];
    if esperados.is_empty() {
        por_criterio[TEXTO].push("a amostra não tem nenhum data-esperado".to_string());
    }

    for w in LARGURAS {
        let texto = match rode(&arquivo, w) {
            Ok(t) => t,
            Err(e) => {
                por_criterio[SUCESSO].push(format!("w{w}: {e}"));
                continue;
            }
        };
        let limpo = sem_ansi(&texto);
        if limpo.trim().is_empty() {
            por_criterio[SUCESSO].push(format!("w{w}: saída vazia"));
            continue;
        }

        let largas: Vec<usize> = limpo.lines().map(UnicodeWidthStr::width).filter(|&l| l > w).collect();
        if let Some(maior) = largas.iter().max() {
            por_criterio[LARGURA]
                .push(format!("w{w}: {} linha(s) passam de {w} colunas (maior: {maior})", largas.len()));
        }

        let achados: Vec<&str> = proibidos.iter().copied().filter(|p| limpo.contains(p)).collect();
        if !achados.is_empty() {
            por_criterio[VAZAMENTO].push(format!("w{w}: aparece {achados:?}"));
        }

        // Texto esperado: exigido na maior largura (em colunas estreitas a quebra legítima parte
        // frases entre colunas lado a lado) e sem diferenciar caixa (`text-transform`).
        let normal = norm(&limpo).to_lowercase();
        let faltando: Vec<&String> = if w == 120 {
            esperados.iter().filter(|e| !normal.contains(&norm(e.as_str()).to_lowercase())).collect()
        } else {
            Vec::new()
        };
        if let Some(primeiro) = faltando.first() {
            por_criterio[TEXTO]
                .push(format!("w{w}: {} faltando, ex.: \"{primeiro}\"", faltando.len()));
        }

        match mdv_html::render(&fonte, &Options { width: w, ..Default::default() }, &Unlimited) {
            Err(e) => por_criterio[CONTRASTE].push(format!("w{w}: erro ao ler: {e}")),
            Ok(doc) => {
                let mut ruins = 0usize;
                let mut pior: Option<(f32, char)> = None;
                for celula in doc.pages.iter().flat_map(|p| p.lines.iter()).flatten() {
                    if celula.ch.is_whitespace() {
                        continue;
                    }
                    if let (Some(fg), Some(bg)) = (celula.face.fg, celula.face.bg) {
                        let razao = contrast(fg, bg);
                        if razao < CONTRASTE_MINIMO {
                            ruins += 1;
                            if pior.is_none_or(|(p, _)| razao < p) {
                                pior = Some((razao, celula.ch));
                            }
                        }
                    }
                }
                if let Some((razao, ch)) = pior {
                    por_criterio[CONTRASTE].push(format!(
                        "w{w}: {ruins} célula(s) abaixo de {CONTRASTE_MINIMO} (pior: {razao:.2} em '{ch}')"
                    ));
                }
            }
        }
    }

    por_criterio
        .into_iter()
        .enumerate()
        .filter(|(_, v)| !v.is_empty())
        .map(|(criterio, v)| Falha { amostra: nome, criterio, mensagem: v.join("; ") })
        .collect()
}

/// Falhas de todas as amostras, coletadas uma única vez (o binário roda 48 vezes no total).
fn relatorio() -> &'static [Falha] {
    static RELATORIO: OnceLock<Vec<Falha>> = OnceLock::new();
    RELATORIO.get_or_init(|| AMOSTRAS.iter().flat_map(|&n| avalia(n)).collect())
}

/// Tabela amostra × critério (✔/✘) seguida da lista de falhas.
fn tabela(falhas: &[Falha]) -> String {
    let mut s = String::from("Régua do corpus HTML (larguras 60, 80, 120)\n\n");
    s.push_str(&format!("{:<22}", "amostra"));
    for c in CRITERIOS {
        s.push_str(&format!("{c:<13}"));
    }
    s.push('\n');
    for nome in AMOSTRAS {
        s.push_str(&format!("{nome:<22}"));
        for c in 0..CRITERIOS.len() {
            let falhou = falhas.iter().any(|f| f.amostra == nome && f.criterio == c);
            s.push_str(&format!("{:<13}", if falhou { "✘" } else { "✔" }));
        }
        s.push('\n');
    }
    if falhas.is_empty() {
        s.push_str("\nNenhuma falha.\n");
    } else {
        s.push_str("\nFalhas:\n");
        for f in falhas {
            s.push_str(&format!("  {} · {}: {}\n", f.amostra, CRITERIOS[f.criterio], f.mensagem));
        }
    }
    s
}

fn confere(criterio: usize) {
    let falhas: Vec<String> = relatorio()
        .iter()
        .filter(|f| f.criterio == criterio)
        .map(|f| format!("{}: {}", f.amostra, f.mensagem))
        .collect();
    assert!(
        falhas.is_empty(),
        "critério {}: {} amostra(s) falharam:\n{}",
        CRITERIOS[criterio],
        falhas.len(),
        falhas.join("\n")
    );
}

#[test]
fn relatorio_do_corpus() {
    let falhas = relatorio();
    let texto = tabela(falhas);
    println!("{texto}");
    assert!(falhas.is_empty(), "{texto}");
}

#[test]
fn criterio1_sucesso_e_saida_nao_vazia() {
    confere(SUCESSO);
}

#[test]
fn criterio2_nenhuma_linha_passa_da_largura() {
    confere(LARGURA);
}

#[test]
fn criterio3_sem_vazamento_de_codigo_ou_marcacao() {
    confere(VAZAMENTO);
}

#[test]
fn criterio4_todo_data_esperado_aparece() {
    confere(TEXTO);
}

#[test]
fn criterio5_contraste_minimo() {
    confere(CONTRASTE);
}

#[test]
fn todas_as_amostras_existem() {
    let faltam: Vec<&str> = AMOSTRAS
        .iter()
        .copied()
        .filter(|n| !root().join("samples/html").join(format!("{n}.html")).is_file())
        .collect();
    assert!(faltam.is_empty(), "amostras ausentes: {faltam:?}");
}

#[test]
fn golden_html_nao_muda() {
    let mut ruins = Vec::new();
    for nome in AMOSTRAS {
        let arquivo = root().join("samples/html").join(format!("{nome}.html"));
        let caminho = root().join("tests/golden/html").join(format!("{nome}.txt"));
        let got = match rode(&arquivo, GOLDEN_LARGURA) {
            Ok(t) => t,
            Err(e) => {
                ruins.push(format!("{nome}: {e}"));
                continue;
            }
        };
        if std::env::var_os("UPDATE_GOLDEN").is_some() {
            fs::create_dir_all(caminho.parent().expect("pasta do golden")).expect("criar tests/golden/html");
            fs::write(&caminho, &got).expect("gravar golden");
            continue;
        }
        match fs::read_to_string(&caminho) {
            Ok(want) if want.replace("\r\n", "\n") == got => {}
            Ok(_) => ruins.push(format!("{nome}: a saída mudou")),
            Err(_) => ruins.push(format!("{nome}: golden ausente (rode com UPDATE_GOLDEN=1)")),
        }
    }
    assert!(ruins.is_empty(), "{}", ruins.join("\n"));
}
