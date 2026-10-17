//! Caracterização das teclas do modo normal e de seus modais mais simples (fase 0 da refatoração).

use super::*;
use std::fs;

fn app(md: &str) -> App {
    let cli = match cli::parse(["x.md".to_string()]) {
        Ok(cli::Parsed::Run(c)) => c,
        _ => panic!("cli de teste"),
    };
    let mut a = App::new(Source::Stdin, md.to_string(), 80, &cli);
    a.rerender(60);
    a.view.height = 10;
    a
}

fn press(a: &mut App, c: KeyCode) -> bool {
    a.on_key(KeyEvent::new(c, KeyModifiers::NONE))
}

fn digita(a: &mut App, texto: &str) {
    for c in texto.chars() {
        press(a, KeyCode::Char(c));
    }
}

/// 60 parágrafos de uma linha: 120 linhas renderizadas, bem mais que a janela de 10.
fn longo() -> String {
    (1..=60).map(|i| format!("linha {i}\n\n")).collect()
}

/// Três seções com 25 parágrafos cada: os títulos ficam bem separados.
fn tres_secoes() -> String {
    ["A", "B", "C"]
        .iter()
        .map(|t| format!("# {t}\n\n{}", "texto\n\n".repeat(25)))
        .collect()
}

#[test]
fn j_k_e_setas_rolam_uma_linha() {
    let mut a = app(&longo());
    press(&mut a, KeyCode::Char('j'));
    assert_eq!(a.view.scroll, 1);
    press(&mut a, KeyCode::Down);
    assert_eq!(a.view.scroll, 2);
    press(&mut a, KeyCode::Char('k'));
    assert_eq!(a.view.scroll, 1);
    press(&mut a, KeyCode::Up);
    press(&mut a, KeyCode::Up);
    assert_eq!(a.view.scroll, 0, "não passa do início");
}

#[test]
fn espaco_f_e_page_rolam_uma_pagina_menos_uma_linha() {
    let mut a = app(&longo());
    press(&mut a, KeyCode::Char(' '));
    assert_eq!(a.view.scroll, 9);
    press(&mut a, KeyCode::Char('f'));
    assert_eq!(a.view.scroll, 18);
    press(&mut a, KeyCode::PageDown);
    assert_eq!(a.view.scroll, 27);
    press(&mut a, KeyCode::Char('b'));
    assert_eq!(a.view.scroll, 18);
    press(&mut a, KeyCode::PageUp);
    assert_eq!(a.view.scroll, 9);
}

#[test]
fn d_e_u_rolam_meia_pagina() {
    let mut a = app(&longo());
    press(&mut a, KeyCode::Char('d'));
    assert_eq!(a.view.scroll, 5);
    press(&mut a, KeyCode::Char('u'));
    assert_eq!(a.view.scroll, 0);
}

#[test]
fn g_e_maiusculo_vao_ao_inicio_e_ao_fim() {
    let mut a = app(&longo());
    let fim = a.rendered.lines.len() - a.view.height;
    press(&mut a, KeyCode::Char('G'));
    assert_eq!(a.view.scroll, fim);
    press(&mut a, KeyCode::Char('g'));
    assert_eq!(a.view.scroll, 0);
    press(&mut a, KeyCode::End);
    assert_eq!(a.view.scroll, fim);
    press(&mut a, KeyCode::Home);
    assert_eq!(a.view.scroll, 0);
}

#[test]
fn colchetes_pulam_de_titulo_em_titulo() {
    let mut a = app(&tres_secoes());
    let h = a.rendered.headings.clone();
    assert_eq!(h.len(), 3);
    press(&mut a, KeyCode::Char(']'));
    assert_eq!(a.view.scroll, h[1]);
    press(&mut a, KeyCode::Char(']'));
    assert_eq!(a.view.scroll, h[2]);
    press(&mut a, KeyCode::Char('['));
    assert_eq!(a.view.scroll, h[1]);
    press(&mut a, KeyCode::Char('['));
    assert_eq!(a.view.scroll, h[0]);
}

#[test]
fn busca_confirma_com_enter_e_n_maiusculo_navegam() {
    let md = format!("{}alvo\n\n{}alvo\n\n", "texto\n\n".repeat(6), "texto\n\n".repeat(30));
    let mut a = app(&md);
    press(&mut a, KeyCode::Char('/'));
    assert_eq!(a.search.input.as_deref(), Some(""));
    digita(&mut a, "alvo");
    press(&mut a, KeyCode::Enter);
    assert_eq!(a.search.input, None, "Enter fecha o prompt");
    assert_eq!(a.search.matches.len(), 2);
    let (primeira, segunda) = (a.search.matches[0], a.search.matches[1]);
    assert_eq!(a.search.current_line(), Some(primeira));
    press(&mut a, KeyCode::Char('n'));
    assert_eq!(a.search.current_line(), Some(segunda));
    press(&mut a, KeyCode::Char('n'));
    assert_eq!(a.search.current_line(), Some(primeira), "dá a volta");
    press(&mut a, KeyCode::Char('N'));
    assert_eq!(a.search.current_line(), Some(segunda), "N volta para trás");
}

#[test]
fn com_o_prompt_de_busca_aberto_as_teclas_viram_texto() {
    let mut a = app(&longo());
    press(&mut a, KeyCode::Char('/'));
    assert!(!press(&mut a, KeyCode::Char('j')), "j não sai nem rola");
    assert_eq!(a.view.scroll, 0);
    assert_eq!(a.search.input.as_deref(), Some("j"));
    press(&mut a, KeyCode::Backspace);
    assert_eq!(a.search.input.as_deref(), Some(""));
    press(&mut a, KeyCode::Esc);
    assert_eq!(a.search.input, None, "Esc fecha o prompt sem sair do programa");
}

#[test]
fn q_e_ctrl_c_saem_e_outras_teclas_nao() {
    let mut a = app("# A\n");
    assert!(!press(&mut a, KeyCode::Char('x')));
    assert!(press(&mut a, KeyCode::Char('q')));
    let mut b = app("# A\n");
    assert!(b.on_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)));
}

#[test]
fn esc_limpa_a_busca_confirmada_e_so_depois_sai() {
    let mut a = app("alvo\n\nresto\n");
    press(&mut a, KeyCode::Char('/'));
    digita(&mut a, "alvo");
    press(&mut a, KeyCode::Enter);
    assert_eq!(a.search.matches.len(), 1);
    assert!(!press(&mut a, KeyCode::Esc), "primeiro Esc só limpa a busca");
    assert!(a.search.matches.is_empty());
    assert!(press(&mut a, KeyCode::Esc), "sem busca, Esc sai");
}

#[test]
fn r_rele_o_arquivo_sob_demanda() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("doc.md");
    fs::write(&p, "# Antes\n").unwrap();
    let cli = match cli::parse([p.display().to_string()]) {
        Ok(cli::Parsed::Run(c)) => c,
        _ => panic!("cli de teste"),
    };
    let mut a = App::new(Source::File(p.clone()), "# Antes\n".to_string(), 80, &cli);
    fs::write(&p, "# Depois\n").unwrap();
    assert!(a.md.contains("Antes"));
    assert!(!press(&mut a, KeyCode::Char('r')));
    assert!(a.md.contains("Depois"), "r relê o arquivo: {:?}", a.md);
}

#[test]
fn r_com_stdin_nao_muda_nada() {
    let mut a = app("# Igual\n");
    assert!(!press(&mut a, KeyCode::Char('r')));
    assert_eq!(a.md, "# Igual\n");
}

#[test]
fn interrogacao_foi_removida_e_nao_faz_nada() {
    let mut a = app(&longo());
    let antes = a.view.scroll;
    assert!(!press(&mut a, KeyCode::Char('?')));
    assert_eq!(a.view.scroll, antes);
    assert!(a.notice.is_none(), "sem aviso de MDV_AI_CMD: {:?}", a.notice);
}

#[test]
fn append_md_acumula_o_texto_na_ordem() {
    let mut a = app("");
    a.append_md("a\n");
    a.append_md("b\nc");
    a.append_md("d\n");
    assert_eq!(a.md, "a\nb\ncd\n");
}

#[test]
fn set_md_troca_o_texto_e_pede_redesenho() {
    let mut a = app("# Antes\n");
    a.dirty = false;
    a.set_md("# Depois\n".to_string());
    assert_eq!(a.md, "# Depois\n");
    assert!(a.dirty);
}

#[test]
fn chaves_e_c_foram_removidas_e_nao_fazem_nada() {
    let mut a = app(&longo());
    for c in ['}', '{', 'c'] {
        assert!(!press(&mut a, KeyCode::Char(c)));
        assert!(a.notice.is_none(), "{c}: {:?}", a.notice);
    }
}

#[test]
fn arquivo_alterado_no_disco_nao_recarrega_sozinho() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("doc.md");
    fs::write(&p, "# Antes\n").unwrap();
    let cli = match cli::parse([p.display().to_string()]) {
        Ok(cli::Parsed::Run(c)) => c,
        _ => panic!("cli de teste"),
    };
    let mut a = App::new(Source::File(p.clone()), "# Antes\n".to_string(), 80, &cli);
    fs::write(&p, "# Depois\n").unwrap();
    a.poll_background();
    assert!(a.md.contains("Antes"), "só a tecla r recarrega: {:?}", a.md);
}

#[test]
fn r_com_arquivo_sumido_mantem_o_documento() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("doc.md");
    fs::write(&p, "# Fica\n").unwrap();
    let cli = match cli::parse([p.display().to_string()]) {
        Ok(cli::Parsed::Run(c)) => c,
        _ => panic!("cli de teste"),
    };
    let mut a = App::new(Source::File(p.clone()), "# Fica\n".to_string(), 80, &cli);
    fs::remove_file(&p).unwrap();
    assert!(!press(&mut a, KeyCode::Char('r')));
    assert_eq!(a.md, "# Fica\n");
}

#[test]
fn z_e_maiusculo_foram_removidos_e_nao_dobram() {
    let mut a = app(&tres_secoes());
    let total = a.rendered.lines.len();
    assert!(!press(&mut a, KeyCode::Char('Z')));
    assert!(!press(&mut a, KeyCode::Char('z')));
    assert_eq!(a.total_lines(), total, "nenhuma linha some");
    assert!(a.notice.is_none(), "{:?}", a.notice);
}

fn ctrl_c(a: &mut App) -> bool {
    a.on_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL))
}

#[test]
fn ctrl_c_sai_no_modo_normal_e_na_busca() {
    let mut a = app(&longo());
    assert!(ctrl_c(&mut a));
    let mut a = app(&longo());
    press(&mut a, KeyCode::Char('/'));
    assert!(ctrl_c(&mut a), "Ctrl-C com o prompt de busca aberto");
}

#[test]
fn busca_aberta_nao_deixa_teclas_vazarem_para_o_modo_normal() {
    let mut a = app(&longo());
    press(&mut a, KeyCode::Char('/'));
    let antes = a.view.scroll;
    for c in ['j', 'q', 'g', 'G', 'w', 'm', 'y'] {
        assert!(!press(&mut a, KeyCode::Char(c)), "{c} saiu com a busca aberta");
    }
    assert_eq!(a.view.scroll, antes);
    assert_eq!(a.search.input.as_deref(), Some("jqgGwmy"));
    assert!(!a.raw, "m não alterna a fonte crua dentro do prompt");
}

#[test]
fn esc_na_busca_cancela_so_o_prompt() {
    let mut a = app(&longo());
    press(&mut a, KeyCode::Char('/'));
    digita(&mut a, "abc");
    assert!(!press(&mut a, KeyCode::Esc));
    assert!(a.search.input.is_none());
    assert!(a.search.query.is_empty());
}

#[test]
fn zoom_de_tabela_captura_as_teclas_e_esc_volta() {
    let md = "| a | b |\n|---|---|\n| 1 | 2 |\n\ntexto\n";
    let mut a = app(md);
    press(&mut a, KeyCode::Char('t'));
    assert!(a.zoom.is_some(), "t abre o zoom da tabela visível");
    let antes = a.view.scroll;
    press(&mut a, KeyCode::Char('j'));
    assert_eq!(a.view.scroll, antes, "j rola o zoom, não o documento");
    assert!(a.zoom.is_some());
    assert!(!press(&mut a, KeyCode::Char('q')), "q fecha o zoom, não sai do app");
    assert!(a.zoom.is_none());
    press(&mut a, KeyCode::Char('t'));
    assert!(ctrl_c(&mut a), "Ctrl-C sai com o zoom aberto");
    let mut a = app(md);
    press(&mut a, KeyCode::Char('t'));
    press(&mut a, KeyCode::Esc);
    assert!(a.zoom.is_none());
}

#[test]
fn confirmacao_de_link_externo_so_aceita_y() {
    let mut a = app("[site](https://example.com)\n");
    a.pending_open = Some("https://example.com".into());
    let antes = a.view.scroll;
    assert!(!press(&mut a, KeyCode::Char('q')), "q cancela, não sai");
    assert!(a.pending_open.is_none());
    assert_eq!(a.view.scroll, antes);
    a.pending_open = Some("https://example.com".into());
    assert!(ctrl_c(&mut a), "Ctrl-C sai com a confirmação aberta");
}

#[test]
fn picker_captura_teclas_e_ctrl_c_sai() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.md"), "# a\n").unwrap();
    let mut a = app("");
    a.picker = Some(picker::Picker::scan(dir.path()).unwrap());
    press(&mut a, KeyCode::Char('j'));
    assert!(a.picker.is_some());
    assert!(ctrl_c(&mut a));
}

#[test]
fn tecla_i_sem_diagrama_so_avisa() {
    let mut a = app("texto\n");
    let antes = a.view.scroll;
    press(&mut a, KeyCode::Char('i'));
    assert!(a.img_zoom.is_none(), "sem diagrama, i só avisa");
    assert!(a.notice.is_some());
    assert_eq!(a.view.scroll, antes);
}

#[test]
fn redimensionar_com_zoom_aberto_nao_fecha_o_zoom() {
    let mut a = app("| a |\n|---|\n| 1 |\n");
    press(&mut a, KeyCode::Char('t'));
    a.rerender(40);
    assert!(a.zoom.is_some());
}
