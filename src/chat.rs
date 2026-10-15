//! `mdv chat`: transcrição de agente (JSONL) → Markdown, que o renderizador já sabe mostrar.
//! Aceita o formato do Claude Code (`{"type":"user|assistant","message":{...}}`) e linhas simples
//! `{"role":"user","content":"..."}`. Raciocínio vira `<think>`; chamadas e resultados de
//! ferramentas viram `<details>` recolhidos.

use crate::mem;
use serde_json::Value;
use std::fmt::Write as _;

/// Linhas maiores que isso não são analisadas (o `Value` ocupa várias vezes o tamanho do texto).
const MAX_LINE_BYTES: usize = 2 * 1024 * 1024;
/// Teto do Markdown gerado.
const MAX_OUT_BYTES: usize = 8 * 1024 * 1024;
/// Teto de caracteres mostrados da entrada de uma ferramenta ou do seu resultado.
const MAX_TOOL_CHARS: usize = 2000;
/// Teto de mensagens convertidas.
const MAX_MESSAGES: usize = 20_000;

/// Primeiros `max` caracteres de `s`, com aviso se cortou.
fn clip(s: &str, max: usize) -> String {
    match s.char_indices().nth(max) {
        Some((i, _)) => format!("{}\n… ({} caracteres omitidos)", &s[..i], s[i..].chars().count()),
        None => s.to_string(),
    }
}

/// Cerca de código mais longa que qualquer sequência de crases em `body`.
fn fence_for(body: &str) -> String {
    let mut longest = 0;
    let mut run = 0;
    for c in body.chars() {
        if c == '`' {
            run += 1;
            longest = longest.max(run);
        } else {
            run = 0;
        }
    }
    "`".repeat((longest + 1).max(3))
}

fn code_block(lang: &str, body: &str) -> String {
    let f = fence_for(body);
    format!("{f}{lang}\n{body}\n{f}\n")
}

/// `<summary>` aceita só uma linha de texto simples.
fn summary_text(s: &str) -> String {
    s.chars().filter(|c| !c.is_control() && !matches!(c, '<' | '>' | '&')).take(120).collect()
}

fn details(summary: &str, body: &str) -> String {
    format!("<details>\n<summary>{}</summary>\n\n{body}\n</details>\n\n", summary_text(summary))
}

fn tool_result_text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Array(items) => items
            .iter()
            .filter_map(|b| b.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("\n"),
        other => other.to_string(),
    }
}

fn block(b: &Value, out: &mut String) {
    match b.get("type").and_then(Value::as_str) {
        Some("text") => {
            if let Some(t) = b.get("text").and_then(Value::as_str) {
                out.push_str(t);
                out.push_str("\n\n");
            }
        }
        Some("thinking") => {
            if let Some(t) = b.get("thinking").and_then(Value::as_str).filter(|t| !t.trim().is_empty()) {
                let _ = write!(out, "<think>\n\n{}\n\n</think>\n\n", clip(t, MAX_TOOL_CHARS * 4));
            }
        }
        Some("tool_use") => {
            let name = b.get("name").and_then(Value::as_str).unwrap_or("ferramenta");
            let input = b.get("input").map(|i| serde_json::to_string_pretty(i).unwrap_or_default()).unwrap_or_default();
            out.push_str(&details(&format!("🔧 {name}"), &code_block("json", &clip(&input, MAX_TOOL_CHARS))));
        }
        Some("tool_result") => {
            let text = b.get("content").map(tool_result_text).unwrap_or_default();
            let bad = b.get("is_error").and_then(Value::as_bool).unwrap_or(false);
            let label = if bad { "↩ resultado (erro)" } else { "↩ resultado" };
            out.push_str(&details(label, &code_block("text", &clip(&text, MAX_TOOL_CHARS))));
        }
        _ => {}
    }
}

/// Papel e conteúdo de uma linha do JSONL, ou `None` se não é uma mensagem.
fn message_of(v: &Value) -> Option<(&str, &Value)> {
    if v.get("isMeta").and_then(Value::as_bool) == Some(true) {
        return None;
    }
    if let Some(m) = v.get("message") {
        let role = m.get("role").and_then(Value::as_str).or_else(|| v.get("type").and_then(Value::as_str))?;
        return Some((role, m.get("content")?));
    }
    Some((v.get("role").and_then(Value::as_str)?, v.get("content")?))
}

/// Converte a transcrição em Markdown.
pub fn to_markdown(text: &str) -> String {
    let mut out = String::new();
    let (mut messages, mut skipped) = (0usize, 0usize);
    let mut last_role = String::new();
    for line in text.lines() {
        if line.trim().is_empty() {
            continue;
        }
        if mem::exceeded() || out.len() > MAX_OUT_BYTES || messages >= MAX_MESSAGES {
            out.push_str("\n_(transcrição cortada: limite de tamanho)_\n");
            break;
        }
        if line.len() > MAX_LINE_BYTES {
            skipped += 1;
            continue;
        }
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            skipped += 1;
            continue;
        };
        let Some((role, content)) = message_of(&v) else { continue };
        let mut body = String::new();
        match content {
            Value::String(s) => {
                body.push_str(s);
                body.push_str("\n\n");
            }
            Value::Array(blocks) => blocks.iter().for_each(|b| block(b, &mut body)),
            _ => {}
        }
        if body.trim().is_empty() {
            continue;
        }
        // Resultado de ferramenta chega como "user", mas não é fala do usuário: continua a vez do agente.
        let only_results = matches!(content, Value::Array(bs) if bs.iter().all(|b| b.get("type").and_then(Value::as_str) == Some("tool_result")));
        let continues = only_results && last_role == "assistant";
        if !continues && role != last_role {
            out.push_str(match role {
                "user" => "## 👤 Usuário\n\n",
                "assistant" => "## 🤖 Assistente\n\n",
                "system" => "## ⚙ Sistema\n\n",
                _ => "## Mensagem\n\n",
            });
        }
        if !continues {
            last_role = role.to_string();
        }
        out.push_str(&body);
        messages += 1;
    }
    if skipped > 0 {
        let _ = write!(out, "\n_({skipped} linha(s) ignorada(s): inválidas ou grandes demais)_\n");
    }
    if out.is_empty() {
        out.push_str("_Nenhuma mensagem encontrada na transcrição._\n");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formato_do_claude_code() {
        let t = concat!(
            r#"{"type":"user","message":{"role":"user","content":"oi"}}"#, "\n",
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"thinking","thinking":"hmm"},{"type":"text","text":"olá"},{"type":"tool_use","name":"Bash","input":{"command":"ls"}}]}}"#, "\n",
            r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","content":"a.txt"}]}}"#, "\n",
            r#"{"type":"summary","summary":"x"}"#, "\n",
        );
        let md = to_markdown(t);
        assert!(md.contains("## 👤 Usuário\n\noi"), "{md}");
        assert!(md.contains("<think>\n\nhmm"), "{md}");
        assert!(md.contains("<summary>🔧 Bash</summary>"), "{md}");
        assert!(md.contains("<summary>↩ resultado</summary>"), "{md}");
        assert_eq!(md.matches("## 🤖").count(), 1);
        assert_eq!(md.matches("## 👤").count(), 1, "resultado não abre nova fala do usuário: {md}");
    }

    #[test]
    fn formato_simples_e_linha_invalida() {
        let md = to_markdown("{\"role\":\"user\",\"content\":\"a\"}\nlixo\n{\"role\":\"assistant\",\"content\":\"b\"}\n");
        assert!(md.contains("## 👤 Usuário\n\na") && md.contains("## 🤖 Assistente\n\nb"), "{md}");
        assert!(md.contains("1 linha(s) ignorada(s)"), "{md}");
    }

    #[test]
    fn cerca_cresce_com_crases_e_texto_longo_e_cortado() {
        let t = format!(
            r#"{{"role":"assistant","content":[{{"type":"tool_result","content":"```x```` {}"}}]}}"#,
            "a".repeat(5000)
        );
        let md = to_markdown(&t);
        assert!(md.contains("`````text"), "cerca de 5 crases: {md}");
        assert!(md.contains("caracteres omitidos"), "{md}");
    }

    #[test]
    fn vazio_avisa() {
        assert!(to_markdown("").contains("Nenhuma mensagem"));
    }
}
