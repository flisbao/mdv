//! Erro de sintaxe de dados estruturados (YAML/JSON) desenhado sobre o texto original: a linha
//! do erro com fundo vermelho e a mensagem logo abaixo.

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthChar;

use crate::mem;
use crate::render::{
    code_row_at, error_rendered, memory_error, render_code_only, sanitize, CodeSpan, Rendered, MAX_HIGHLIGHT_BYTES,
};

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
    let (mut start, mut end) = (0, text.len());
    let mut at = 0;
    for (i, piece) in text.split_inclusive('\n').enumerate() {
        if let Err(e) = mem::check() {
            return error_rendered(&e.to_string());
        }
        (start, end) = (at, at + piece.len());
        if i + 1 >= err.line.max(1) {
            break;
        }
        at += piece.len();
    }

    // Cada `render_code_only` tem o próprio orçamento de realce: acima dele, só a linha do erro sai
    // colorida, para o texto grande não custar três orçamentos a cada re-render.
    let rest = if text.len() > MAX_HIGHLIGHT_BYTES { "" } else { lang };
    let before = render_code_only(&text[..start], rest, width);
    let mid = render_code_only(&text[start..end], lang, width);
    let after = render_code_only(&text[end..], rest, width);
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
    // A linha toda fica pintada, mas a mensagem vem logo depois da parte quebrada que tem a
    // coluna do erro: numa linha minificada longa, o fim dela pode estar milhares de linhas abaixo.
    let mut body = mid.lines;
    body.pop();
    let parts = body.len().saturating_sub(1);
    let row = code_row_at(&text[start..end], err.col, width).min(parts.saturating_sub(1));
    let err_line = out.lines.len() + row;
    let mut body = body.into_iter().skip(1).map(paint);
    out.lines.extend(body.by_ref().take(row + 1));
    out.lines.extend(msg_lines);
    out.lines.extend(body);
    out.lines.extend(after.lines.into_iter().skip(1));
    out.focus = Some(err_line.saturating_sub(FOCUS_CONTEXT));
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
    let mut flush = |cur: &mut String, w: &mut usize| {
        lines.push(Line::from(Span::styled(std::mem::take(cur), st)));
        cur.push_str("  ");
        *w = 2;
    };
    for word in sanitize(&full).split(' ') {
        let ww: usize = word.chars().map(|c| c.width().unwrap_or(0)).sum();
        let vazia = cur.trim().is_empty();
        let sep = usize::from(!vazia);
        if w + sep + ww > avail && !vazia {
            flush(&mut cur, &mut w);
        } else if sep == 1 {
            cur.push(' ');
            w += 1;
        }
        // Palavra maior que a linha (um caminho, por exemplo): só então quebra no meio dela.
        for ch in word.chars() {
            let cw = ch.width().unwrap_or(0);
            if w + cw > avail && !cur.trim().is_empty() {
                flush(&mut cur, &mut w);
            }
            cur.push(ch);
            w += cw;
        }
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
        // Coluna 200 é o último caractere: cai na última parte, e a mensagem vem depois dela.
        let r = render_with_error(&src, "yaml", 40, &err(2, 200, "ruim"));
        let partes: Vec<usize> =
            r.lines.iter().enumerate().filter(|(_, l)| text(l).contains('x')).map(|(n, _)| n).collect();
        assert!(partes.len() > 1);
        assert!(partes.iter().all(|&n| r.lines[n].spans.iter().all(|s| s.style.bg == Some(ERR_BG))));
        let marcadas = r.lines.iter().filter(|l| has_err_bg(l)).count();
        assert_eq!(marcadas, partes.len());
        let ultima = *partes.last().unwrap();
        assert!(text(&r.lines[ultima + 1]).contains("✗ linha 2, coluna 200: ruim"));
    }

    /// Linha longa (minificada) com o erro numa coluna alta: a mensagem vem logo depois da parte
    /// quebrada que tem a coluna, e o `focus` fica a até `FOCUS_CONTEXT` linhas acima dela.
    fn mensagem_segue_a_parte_da_coluna(prefixo: &str) {
        let col = prefixo.chars().count() + 1;
        let longa = format!("{prefixo}Y{}", "x".repeat(1000));
        let src = format!("a: 1\n{longa}\nc: 3\n");
        let r = render_with_error(&src, "yaml", 40, &err(2, col, "ruim"));
        let m = r.lines.iter().position(|l| text(l).contains('✗')).expect("mensagem");
        assert!(text(&r.lines[m]).contains(&format!("✗ linha 2, coluna {col}: ruim")));
        assert!(text(&r.lines[m - 1]).contains('Y'), "a parte antes da mensagem: {}", text(&r.lines[m - 1]));
        assert!(has_err_bg(&r.lines[m - 1]) && has_err_bg(&r.lines[m + 1]), "a linha toda segue pintada");
        let n = r.focus.expect("focus");
        assert!(n >= (m - 1).saturating_sub(FOCUS_CONTEXT) && n < m, "focus {n}, mensagem em {m}");
    }

    #[test]
    fn linha_longa_poe_a_mensagem_na_parte_do_erro() {
        mensagem_segue_a_parte_da_coluna(&"x".repeat(2999));
    }

    #[test]
    fn linha_longa_com_tabulacao_e_caractere_largo() {
        // Tabulação vira 4 colunas e `界` ocupa 2, como no desenho do bloco de código.
        mensagem_segue_a_parte_da_coluna(&format!("\t{}\t{}", "界".repeat(700), "x".repeat(601)));
    }

    #[test]
    fn coluna_alem_da_linha_usa_a_ultima_parte() {
        let src = format!("a: 1\n{}\nc: 3\n", "x".repeat(200));
        let r = render_with_error(&src, "yaml", 40, &err(2, 5000, "ruim"));
        let m = r.lines.iter().position(|l| text(l).contains('✗')).unwrap();
        assert!(has_err_bg(&r.lines[m - 1]) && !has_err_bg(&r.lines[m + 1]));
    }

    /// Cores distintas do conteúdo (sem a moldura `│ ` nem o preenchimento) das linhas `linhas`.
    fn cores(r: &Rendered, linhas: std::ops::Range<usize>) -> std::collections::HashSet<String> {
        r.lines[linhas]
            .iter()
            .flat_map(|l| l.spans.iter().skip(1))
            .filter(|s| !s.content.trim().is_empty())
            .filter_map(|s| s.style.fg.map(|c| format!("{c:?}")))
            .collect()
    }

    /// `n` linhas `chave: valor` com uma linha quebrada no meio; devolve o texto e a linha do erro.
    fn yaml_com_erro_no_meio(n: usize) -> (String, usize) {
        let mut t: String = (0..n / 2).map(|i| format!("k{i}: \"v\" # n\n")).collect();
        t.push_str("b: [\n");
        t.extend((0..n / 2).map(|i| format!("j{i}: true # n\n")));
        (t, n / 2 + 1)
    }

    #[test]
    fn texto_grande_so_colore_a_linha_do_erro() {
        // Acima do orçamento de realce (256 KiB): ~280 KiB, umas 20 mil linhas curtas.
        let (src, linha) = yaml_com_erro_no_meio(20_000);
        assert!(src.len() > MAX_HIGHLIGHT_BYTES && src.len() < 2 * MAX_HIGHLIGHT_BYTES, "{}", src.len());
        let t0 = std::time::Instant::now();
        let r = render_with_error(&src, "yaml", 60, &err(linha, 4, "ruim"));
        let gasto = t0.elapsed();
        let i = r.lines.iter().position(|l| has_err_bg(l)).expect("linha do erro");
        assert!(text(&r.lines[i]).contains("b: ["));
        assert_eq!(cores(&r, 1..i).len(), 1, "antes do erro sem realce");
        assert_eq!(cores(&r, i + 2..r.lines.len() - 1).len(), 1, "depois do erro sem realce");
        assert!(cores(&r, i..i + 1).len() > 1, "a linha do erro continua colorida");
        // Em debug: ~0,3 s com o conserto, ~6 s antes; a folga cobre a carga dos testes em paralelo.
        assert!(gasto.as_secs_f64() < 2.0, "levou {gasto:?}");
    }

    #[test]
    fn texto_pequeno_colore_antes_e_depois_do_erro() {
        let (src, linha) = yaml_com_erro_no_meio(20);
        let r = render_with_error(&src, "yaml", 60, &err(linha, 4, "ruim"));
        let i = r.lines.iter().position(|l| has_err_bg(l)).unwrap();
        assert!(cores(&r, 1..i).len() > 1 && cores(&r, i + 2..r.lines.len() - 1).len() > 1);
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
    fn mensagem_longa_quebra_entre_palavras() {
        let msg = "esperado vírgula ou chave depois do valor anterior";
        let r = render_with_error("a: 1\n", "yaml", 24, &err(1, 1, msg));
        let ms: Vec<String> = r.lines.iter().map(text).filter(|t| t.contains('✗') || t.starts_with("  ")).collect();
        assert!(ms.len() > 1, "{ms:?}");
        assert!(ms.iter().all(|t| t.chars().count() <= 24), "{ms:?}");
        let junto = ms.iter().map(|t| t.trim()).collect::<Vec<_>>().join(" ");
        assert_eq!(junto, format!("✗ linha 1, coluna 1: {msg}"));
    }

    #[test]
    fn palavra_maior_que_a_linha_quebra_no_meio() {
        let r = render_with_error("a: 1\n", "yaml", 10, &err(1, 1, &"z".repeat(30)));
        let n = r.lines.iter().filter(|l| text(l).contains('z')).count();
        assert!(n >= 3);
    }

    #[test]
    fn linha_um_sem_quebra_final_e_linha_zero() {
        let r = render_with_error("a: [", "yaml", 60, &err(1, 4, "ruim"));
        assert!(r.lines.iter().any(|l| has_err_bg(l) && text(l).contains("a: [")));
        let r = render_with_error("a: 1\r\nb: 2\r\n", "yaml", 60, &err(0, 1, "ruim"));
        let i = r.lines.iter().position(|l| has_err_bg(l)).unwrap();
        assert!(text(&r.lines[i]).contains("a: 1"));
    }

    #[test]
    fn texto_vazio_so_mostra_a_mensagem() {
        let r = render_with_error("", "yaml", 60, &err(1, 1, "documento vazio"));
        assert_eq!(r.lines.len(), 1);
        assert!(text(&r.lines[0]).contains("✗ linha 1, coluna 1: documento vazio"));
    }
}
