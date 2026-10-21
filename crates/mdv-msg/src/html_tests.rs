use super::*;
use crate::{Unlimited, MAX_HTML_BYTES};

fn md(h: &str) -> String { to_markdown(h, &Unlimited).unwrap() }

/// Conta os `|` de uma linha que não estão escapados por `\`.
fn pipes_sem_escape(l: &str) -> usize {
    let cs: Vec<char> = l.chars().collect();
    (0..cs.len()).filter(|&i| cs[i] == '|' && (i == 0 || cs[i - 1] != '\\')).count()
}

#[test]
fn paragrafos_e_enfase() {
    assert_eq!(md("<p>Olá <b>Ana</b>,</p><p>segue <i>o</i> relatório.</p>"), "Olá **Ana**,\n\nsegue *o* relatório.");
}
#[test]
fn tabela_de_dados_vira_gfm() {
    assert_eq!(md("<table><tr><td>Sistema</td><td>GB</td></tr><tr><td>CAM</td><td>115,8</td></tr></table>"),
        "| Sistema | GB |\n| --- | --- |\n| CAM | 115,8 |");
}
#[test]
fn tabela_de_layout_aninhada_vira_paragrafos_e_dados_viram_gfm() {
    let h = "<table><tr><td><p>Prezados,</p><table><tr><td>a</td><td>b</td></tr><tr><td>1</td><td>2</td></tr></table><p>Abs</p></td></tr></table>";
    assert_eq!(md(h), "Prezados,\n\n| a | b |\n| --- | --- |\n| 1 | 2 |\n\nAbs");
}
#[test]
fn colspan_mantem_o_numero_de_colunas() {
    assert_eq!(md("<table><tr><td colspan=2>x</td></tr><tr><td>a</td><td>b</td></tr></table>"),
        "| x |  |\n| --- | --- |\n| a | b |");
}
#[test]
fn barra_vertical_na_celula_e_escapada() {
    assert_eq!(md("<table><tr><td>a|b</td><td>c</td></tr><tr><td>1</td><td>2</td></tr></table>"),
        "| a\\|b | c |\n| --- | --- |\n| 1 | 2 |");
}
#[test]
fn respostas_encadeadas_aprofundam_a_citacao() {
    let h = r#"<div><p>Resposta 2</p><hr><div id="divRplyFwdMsg"><b>De:</b> Ana<br><b>Enviado:</b> ontem</div><p>Resposta 1</p><div style="border:none;border-top:solid #E1E1E1 1.0pt"><p><b>From:</b> Bia</p></div><p>Original</p></div>"#;
    assert_eq!(md(h), "Resposta 2\n\n> **De:** Ana\\\n> **Enviado:** ontem\n>\n> Resposta 1\n>\n> > **From:** Bia\n> >\n> > Original");
}
#[test]
fn imagem_cid_vira_rotulo() {
    assert_eq!(md(r#"<p><img src="cid:logo.png@01" alt="Logo"></p>"#), "[imagem: Logo]");
}
#[test]
fn estilo_script_e_comentario_condicional_somem() {
    assert_eq!(md("<style>p{}</style><!--[if mso]>x<![endif]--><p>a</p>"), "a");
}
#[test]
fn texto_com_markdown_e_escapado() {
    assert_eq!(md("<p># não é título *nem* ênfase</p>"), "\\# não é título \\*nem\\* ênfase");
}
#[test]
fn links_e_listas() {
    assert_eq!(md("<ul><li>a</li><li><a href=\"https://x.y\">x</a></li></ul>"), "- a\n- [x](https://x.y)");
}
#[test]
fn html_acima_do_teto_e_recusado() {
    let e = to_markdown(&"a".repeat(MAX_HTML_BYTES + 1), &Unlimited).unwrap_err();
    assert!(e.contains("grande demais"), "{e}");
}
#[test]
fn aninhamento_profundo_nao_estoura_a_pilha() {
    let r = md(&("<div>".repeat(5000) + "x"));
    assert!(r.contains('x'), "{r}");
}

#[test]
fn cabecalho_embrulhado_do_outlook_desktop_cita_o_corpo() {
    let h = r#"<div><p>Resp</p><div><div style="border:none;border-top:solid #E1E1E1 1.0pt"><p><b>From:</b> Bia</p></div></div><p>Original</p></div>"#;
    assert_eq!(md(h), "Resp\n\n> **From:** Bia\n>\n> Original");
}
#[test]
fn owa_appendonsend_abre_um_nivel_so() {
    let h = r#"<p>Resp</p><div id="appendonsend"></div><hr><div id="divRplyFwdMsg"><b>De:</b> Ana</div><div>Original</div>"#;
    assert_eq!(md(h), "Resp\n\n> **De:** Ana\n>\n> Original");
}
#[test]
fn hr_dentro_do_cabecalho_de_resposta_e_descartado() {
    let h = r#"<div><p>Resp</p><div id="divRplyFwdMsg"><hr><p><b>De:</b> Ana</p></div><div>Original</div></div>"#;
    assert_eq!(md(h), "Resp\n\n> **De:** Ana\n>\n> Original");
}
#[test]
fn citacao_limitada_a_20_niveis() {
    let mut h = String::from("<div>");
    for k in 0..25 {
        h += &format!(r#"<div id="divRplyFwdMsg"><b>De:</b> n{k}</div><p>t{k}</p>"#);
    }
    h += "</div>";
    let r = md(&h);
    let max = r
        .lines()
        .map(|l| l.chars().take_while(|c| *c == '>' || *c == ' ').filter(|c| *c == '>').count())
        .max()
        .unwrap_or(0);
    assert_eq!(max, 20, "{r}");
    assert!(r.contains("n24"), "{r}");
}
#[test]
fn saida_acima_do_teto_e_recusada() {
    let h = "<p>abcdefghijklmnopqrstuvwxyz</p>".repeat(4);
    let e = to_markdown_com_teto(&h, &Unlimited, 64).unwrap_err();
    assert!(e.contains("e-mail grande demais para converter"), "{e}");
}
#[test]
fn exclamacao_antes_de_link_nao_vira_imagem() {
    let r = md(r#"<p>oi!<a href="https://x.y">x</a></p>"#);
    assert!(!r.contains("!["), "{r}");
    assert_eq!(r, "oi&#33;[x](https://x.y)");
}
#[test]
fn celula_com_titulo_pre_imagem_e_autolink_nao_quebra_a_tabela() {
    let h = r#"<table><tr><td><h3>T|t</h3></td><td><pre>p|q</pre></td></tr><tr><td><img src="https://x.y/i.png" alt="i|j"></td><td><a href="https://x.y/a|b">https://x.y/a|b</a></td></tr></table>"#;
    let r = md(h);
    let linhas: Vec<&str> = r.lines().collect();
    assert_eq!(linhas.len(), 3, "{r}");
    for l in &linhas {
        assert_eq!(pipes_sem_escape(l), 3, "{r}");
    }
    assert!(r.contains("T\\|t"), "{r}");
}
#[test]
fn ol_start_enorme_nao_entra_em_panico() {
    assert_eq!(md(r#"<ol start="18446744073709551615"><li>a</li></ol>"#), "999999000. a");
}
#[test]
fn html_aninhado_demais_e_recusado_rapido() {
    let h = "<center>".repeat(50_000);
    let inicio = std::time::Instant::now();
    let e = to_markdown(&h, &Unlimited).unwrap_err();
    assert!(e.contains("aninhado demais"), "{e}");
    assert!(inicio.elapsed() < std::time::Duration::from_secs(2), "{:?}", inicio.elapsed());
}
#[test]
fn html_com_muitas_tags_irmas_nao_e_recusado() {
    assert_eq!(md(&"<p>a</p>".repeat(50_000)), "a\n\n".repeat(49_999) + "a");
}
#[test]
fn celula_com_varios_paragrafos_vira_layout_sem_perder_texto() {
    let h = "<table><tr><td><p>um</p><p>dois</p><p>tres final</p></td><td>x</td></tr><tr><td>1</td><td>2</td></tr></table>";
    let r = md(h);
    assert!(r.contains("tres final"), "{r}");
    assert!(!r.contains('…'), "{r}");
    assert!(!r.contains("| --- |"), "{r}");
}
#[test]
fn celula_longa_vira_layout_sem_perder_texto() {
    let longo = "palavra ".repeat(75);
    let h = format!("<table><tr><td>{longo}FIM</td><td>x</td></tr><tr><td>1</td><td>2</td></tr></table>");
    let r = md(&h);
    assert!(r.contains("FIM"), "{r}");
    assert!(!r.contains('…'), "{r}");
}
