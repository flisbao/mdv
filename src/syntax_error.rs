//! Erro de sintaxe de dados estruturados (YAML/JSON) desenhado sobre o texto original: a linha
//! do erro com fundo vermelho e a mensagem logo abaixo.

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthChar;

use crate::mem;
use crate::render::{error_rendered, memory_error, render_code_only, sanitize, CodeSpan, Rendered};

/// Fundo da linha com o erro.
pub const ERR_BG: Color = Color::Rgb(90, 30, 30);
/// Linhas de contexto que ficam acima do erro ao abrir (`Rendered.focus`).
const FOCUS_CONTEXT: usize = 3;

/// Erro de sintaxe com posição (`line` e `col` começam em 1) e mensagem sem a posição.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyntaxError {
    pub line: usize,
    pub col: usize,
    pub message: String,
}

/// Desenha `text` como código `lang` com a linha do erro destacada e a mensagem logo depois.
/// `line` além do fim do texto vale a última linha; texto vazio vira só a mensagem.
pub fn render_with_error(text: &str, lang: &str, width: usize, err: &SyntaxError) -> Rendered {
    let msg_lines = message_lines(err, width);
    if text.is_empty() {
        return Rendered { lines: msg_lines, ..Rendered::default() };
    }
    // Posição em bytes onde começa a linha do erro (`start`) e a seguinte (`end`).
    let (mut start, mut end, mut found) = (0, text.len(), false);
    let mut at = 0;
    for (i, piece) in text.split_inclusive('\n').enumerate() {
        if mem::exceeded() {
            return error_rendered("limite de memória excedido");
        }
        (start, end, found) = (at, at + piece.len(), true);
        if i + 1 >= err.line.max(1) {
            break;
        }
        at += piece.len();
    }
    if !found {
        return Rendered { lines: msg_lines, ..Rendered::default() };
    }

    let before = render_code_only(&text[..start], lang, width);
    let mid = render_code_only(&text[start..end], lang, width);
    let after = render_code_only(&text[end..], lang, width);
    if let Some(e) = memory_error() {
        return e;
    }

    let mut out = Rendered {
        code_overflow: before.code_overflow.max(mid.code_overflow).max(after.code_overflow),
        ..Rendered::default()
    };
    // `before` mantém o cabeçalho; `after`, o rodapé; as molduras que sobram saem.
    let mut head = before.lines;
    head.pop();
    out.lines = head;
    let first_err = out.lines.len();
    let mut body = mid.lines;
    body.pop();
    out.lines.extend(body.into_iter().skip(1).map(paint));
    out.lines.extend(msg_lines);
    out.lines.extend(after.lines.into_iter().skip(1));
    out.focus = Some(first_err.saturating_sub(FOCUS_CONTEXT));
    out.code_blocks.push(CodeSpan {
        start: 0,
        end: out.lines.len(),
        lang: lang.to_string(),
        text: sanitize(text).into_owned(),
    });
    out
}

/// Fundo de erro em todos os trechos da linha (inclusive a barra `│` e o preenchimento).
fn paint(mut line: Line<'static>) -> Line<'static> {
    for sp in &mut line.spans {
        sp.style = sp.style.bg(ERR_BG);
    }
    line
}

/// `✗ linha L, coluna C: mensagem`, vermelha e em negrito, quebrada em `width` colunas
/// (as continuações levam dois espaços de recuo).
fn message_lines(err: &SyntaxError, width: usize) -> Vec<Line<'static>> {
    let st = Style::default().fg(Color::Rgb(248, 81, 73)).add_modifier(Modifier::BOLD);
    // Quebra de linha ou tabulação na mensagem (vinda do parser) não pode vazar para a tela.
    let message = err.message.split_whitespace().collect::<Vec<_>>().join(" ");
    let full = format!("✗ linha {}, coluna {}: {}", err.line, err.col, message);
    let avail = width.max(8);
    let mut lines = Vec::new();
    let mut cur = String::new();
    let mut w = 0;
    for ch in sanitize(&full).chars() {
        let cw = ch.width().unwrap_or(0);
        if w + cw > avail && !cur.is_empty() {
            lines.push(Line::from(Span::styled(std::mem::take(&mut cur), st)));
            cur.push_str("  ");
            w = 2;
        }
        cur.push(ch);
        w += cw;
    }
    lines.push(Line::from(Span::styled(cur, st)));
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::text::Line;

    fn text(l: &Line) -> String {
        l.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    fn has_err_bg(l: &Line) -> bool {
        l.spans.iter().any(|s| s.style.bg == Some(ERR_BG))
    }

    fn err(line: usize, col: usize, message: &str) -> SyntaxError {
        SyntaxError { line, col, message: message.into() }
    }

    #[test]
    fn destaca_a_linha_do_erro_e_poe_a_mensagem_depois() {
        let r = render_with_error("a: 1\nb: [\nc: 3\n", "yaml", 60, &err(2, 4, "colchete sem fechar"));
        let i = r.lines.iter().position(|l| text(l).contains("b: [")).expect("linha do erro");
        assert!(r.lines[i].spans.iter().all(|s| s.style.bg == Some(ERR_BG)));
        assert!(text(&r.lines[i + 1]).contains("✗ linha 2, coluna 4: colchete sem fechar"));
        let marcadas: Vec<usize> =
            r.lines.iter().enumerate().filter(|(_, l)| has_err_bg(l)).map(|(n, _)| n).collect();
        assert_eq!(marcadas, vec![i]);
        let conta = |s: &str| r.lines.iter().filter(|l| text(l).contains(s)).count();
        assert_eq!(conta("╭─"), 1);
        assert_eq!(conta("╰─"), 1);
        assert!(text(&r.lines[0]).contains("╭─"));
        assert!(text(r.lines.last().unwrap()).contains("╰─"));
    }

    #[test]
    fn linha_do_erro_alem_do_fim_e_limitada() {
        let r = render_with_error("a: 1\nb: 2\n", "yaml", 60, &err(99, 1, "fim inesperado"));
        let i = r.lines.iter().position(|l| text(l).contains("b: 2")).unwrap();
        assert!(has_err_bg(&r.lines[i]));
        assert!(text(&r.lines[i + 1]).contains("✗ linha 99, coluna 1: fim inesperado"));
    }

    #[test]
    fn linha_larga_destaca_todas_as_partes() {
        let larga = "x".repeat(200);
        let src = format!("a: 1\n{larga}\nc: 3\n");
        let r = render_with_error(&src, "yaml", 40, &err(2, 1, "ruim"));
        let partes: Vec<usize> =
            r.lines.iter().enumerate().filter(|(_, l)| text(l).contains('x')).map(|(n, _)| n).collect();
        assert!(partes.len() > 1);
        assert!(partes.iter().all(|&n| r.lines[n].spans.iter().all(|s| s.style.bg == Some(ERR_BG))));
        let marcadas = r.lines.iter().filter(|l| has_err_bg(l)).count();
        assert_eq!(marcadas, partes.len());
        let ultima = *partes.last().unwrap();
        assert!(text(&r.lines[ultima + 1]).contains("✗ linha 2, coluna 1: ruim"));
    }

    #[test]
    fn focus_aponta_para_perto_do_erro() {
        let src: String = (1..=100).map(|n| format!("k{n}: {n}\n")).collect();
        let r = render_with_error(&src, "yaml", 60, &err(80, 1, "ruim"));
        let i = r.lines.iter().position(|l| has_err_bg(l)).unwrap();
        let n = r.focus.expect("focus");
        assert!(n >= i.saturating_sub(3) && n <= i, "focus {n}, erro em {i}");
    }

    #[test]
    fn mensagem_nao_leva_quebra_de_linha_nem_tabulacao() {
        let r = render_with_error("a: 1\n", "yaml", 60, &err(1, 1, "ruim\n\tmesmo\r\ndepois"));
        let i = r.lines.iter().position(|l| text(l).contains('✗')).unwrap();
        let t = text(&r.lines[i]);
        assert!(t.contains("✗ linha 1, coluna 1: ruim mesmo depois"), "{t}");
        assert!(r.lines.iter().all(|l| !text(l).contains(['\n', '\t', '\r'])));
    }

    #[test]
    fn texto_vazio_so_mostra_a_mensagem() {
        let r = render_with_error("", "yaml", 60, &err(1, 1, "documento vazio"));
        assert_eq!(r.lines.len(), 1);
        assert!(text(&r.lines[0]).contains("✗ linha 1, coluna 1: documento vazio"));
    }
}
