use super::testutil::*;
use super::*;

fn assert_no_control_chars(md: &str) {
    let visible = strip_sgr(&plain_output(md));
    let bad: Vec<char> = visible.chars().filter(|c| c.is_control() && *c != '\n').collect();
    assert!(bad.is_empty(), "caracteres de controle na saída: {bad:?}");
}

#[test]
fn sanitiza_escape_em_titulo() {
    assert_no_control_chars("# ti\x1b]0;pwn\x07tulo\n");
}

#[test]
fn sanitiza_escape_em_texto_e_codigo_inline() {
    assert_no_control_chars("texto \x1b[2J e `code \x1b]52;c;AAAA\x07`\n");
}

#[test]
fn sanitiza_escape_em_url_de_link() {
    assert_no_control_chars("[x](http://a/\x1b]0;pwn\x07b)\n");
}

#[test]
fn sanitiza_escape_em_bloco_de_codigo_e_linguagem() {
    assert_no_control_chars("```rs\x1b[2J\nlet a = \"\x1b[2J\";\n```\n");
    assert_no_control_chars("    indentado \x1b[31m\n");
}

#[test]
fn sanitiza_escape_em_html() {
    assert_no_control_chars("<div>\x1b[2J</div>\n\ntexto <b\x1b[2J>x</b>\n");
}

#[test]
fn sanitiza_escape_vindo_de_entidade() {
    assert_no_control_chars("a &#27;[2J b &#x9b;31m\n");
}

#[test]
fn sanitiza_controles_c1_e_retorno_de_carro_solto() {
    assert_no_control_chars("a\u{9b}31m b\rc\n");
}

#[test]
fn largura_enorme_e_limitada_em_render() {
    let out = render("---\n\n# t\n", usize::MAX);
    let widest = out.lines.iter().map(|l| l.width()).max().unwrap_or(0);
    assert!(widest <= MAX_WIDTH, "linha com {widest} colunas");
}

#[test]
fn largura_minima_e_respeitada_em_render() {
    let out = render("---\n", 0);
    assert_eq!(out.lines[0].width(), MIN_WIDTH);
}

/// Algoritmo original (uma unidade por iteração), usado como referência.
fn reference_widths(natural: &[usize], avail: usize) -> Vec<usize> {
    let mut widths = natural.to_vec();
    let n = widths.len();
    let mut total: usize = widths.iter().sum::<usize>() + 3 * n + 1;
    while total > avail {
        let (idx, &mx) = widths.iter().enumerate().max_by_key(|(_, w)| **w).unwrap();
        if mx <= 3 {
            break;
        }
        widths[idx] -= 1;
        total -= 1;
    }
    widths
}

#[test]
fn larguras_de_coluna_iguais_ao_algoritmo_de_referencia() {
    let mut seed = 12345u64;
    let mut next = |m: u64| {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((seed >> 33) % m) as usize
    };
    for _ in 0..2000 {
        let ncols = 1 + next(8);
        let natural: Vec<usize> = (0..ncols).map(|_| 1 + next(40)).collect();
        let avail = next(120);
        assert_eq!(
            compute_column_widths(&natural, avail),
            reference_widths(&natural, avail),
            "natural={natural:?} avail={avail}"
        );
    }
}

#[test]
fn larguras_de_coluna_nao_dependem_do_tamanho_da_celula() {
    let mut natural = vec![5usize; 3000];
    natural[1500] = 3_000_000;
    let started = std::time::Instant::now();
    let widths = compute_column_widths(&natural, 100);
    assert!(started.elapsed() < std::time::Duration::from_millis(200));
    assert_eq!(widths.len(), 3000);
    assert!(widths.iter().all(|&w| w >= 3));
}

#[test]
fn tabela_larga_renderiza_em_tempo_limitado() {
    let ncols = 3000;
    let header = format!("|{}\n", " h |".repeat(ncols));
    let sep = format!("|{}\n", "---|".repeat(ncols));
    let mut cells = vec!["c".to_string(); ncols];
    cells[ncols / 2] = "x".repeat(3_000_000);
    let row = format!("| {} |\n", cells.join(" | "));
    let md = format!("{header}{sep}{row}");
    let started = std::time::Instant::now();
    let out = render(&md, 80);
    assert!(!out.lines.is_empty());
    assert!(started.elapsed() < std::time::Duration::from_secs(5));
}

    fn line_to_string(l: &Line<'_>) -> String {
        l.spans.iter().map(|s| s.content.as_ref()).collect()
    }

#[test]
fn item_iniciado_por_bloco_de_codigo_mantem_marcador() {
    let lines = rendered_text("- ```rust\n  let a = 1;\n  ```\n");
    assert!(lines[0].starts_with("• "), "primeira linha: {:?}", lines[0]);
    assert!(!lines[1].contains('•'));
}

#[test]
fn item_ordenado_iniciado_por_bloco_de_codigo_mantem_numero() {
    let lines = rendered_text("1. ```\n   x\n   ```\n2. dois\n");
    assert!(lines[0].starts_with("1. "), "primeira linha: {:?}", lines[0]);
    assert!(lines.iter().any(|l| l.starts_with("2. ")));
}

#[test]
fn item_iniciado_por_tabela_mantem_marcador() {
    let lines = rendered_text("- | a | b |\n  |---|---|\n  | 1 | 2 |\n");
    assert!(lines[0].starts_with("• "), "primeira linha: {:?}", lines[0]);
    assert_eq!(lines.iter().filter(|l| l.contains('•')).count(), 1);
}

#[test]
fn marcador_sai_uma_unica_vez_por_item() {
    let lines = rendered_text("- texto longo que quebra em varias linhas porque e bem comprido mesmo e passa de oitenta colunas facilmente\n");
    assert_eq!(lines.iter().filter(|l| l.contains('•')).count(), 1);
}

#[test]
fn preserva_texto_comum_e_tabulacao_em_codigo() {
    let out = strip_sgr(&plain_output("```\n\tx\r\ny\n```\n"));
    assert!(out.contains("    x"), "tab deveria virar espaços: {out:?}");
    assert!(out.contains('y'));
}

#[test]
fn regua_respeita_prefixo_de_citacao_e_lista() {
    assert_fits("> ---\n", 30);
    assert_fits("- a\n\n  ---\n", 30);
    assert_fits("> - a\n>\n>   ---\n", 30);
    let lines = rendered_text_at("> ---\n", 30);
    assert!(lines.iter().any(|l| l.starts_with("▎ ─")), "{lines:?}");
    let lines = rendered_text_at("- ---x\n\n  ---\n", 30);
    assert!(lines.iter().any(|l| l.starts_with("  ─")), "{lines:?}");
}

#[test]
fn html_em_bloco_respeita_prefixo_e_largura() {
    let long = "x".repeat(200);
    assert_fits(&format!("> <div>{long}</div>\n"), 40);
    assert_fits(&format!("- item\n\n  <div>{long}</div>\n"), 40);
    assert_fits(&format!("<div>{long}</div>\n"), 40);
}

#[test]
fn titulos_respeitam_prefixo_de_citacao_e_lista() {
    assert_fits("> # um titulo bem comprido que precisa quebrar em varias linhas\n", 30);
    assert_fits("- ## um titulo bem comprido que precisa quebrar em varias linhas\n", 30);
    assert_fits("> ### um titulo bem comprido que precisa quebrar em varias linhas\n", 30);
}

#[test]
fn titulo_em_citacao_mantem_barra_na_primeira_linha() {
    let lines = rendered_text("> # titulo\n");
    assert!(lines.iter().any(|l| l.starts_with("▎ ") && l.contains("titulo")), "{lines:?}");
}

#[test]
fn tabela_com_muitas_colunas_cabe_na_largura() {
    let n = 30;
    let header = format!("|{}\n", " h |".repeat(n));
    let sep = format!("|{}\n", "---|".repeat(n));
    let row = format!("|{}\n", " c |".repeat(n));
    assert_fits(&format!("{header}{sep}{row}"), 20);
    assert_fits(&format!("> {header}> {sep}> {row}"), 20);
}

    const FRASE_LONGA: &str = "uma descricao bem longa que precisa quebrar em varias linhas dentro da celula";

    fn tabela_de_uma_celula(texto: &str, alinhamento: &str) -> String {
        format!("| titulo |\n|{alinhamento}|\n| {texto} |\n")
    }

    #[test]
    fn celula_longa_quebra_em_varias_linhas() {
        let md = format!("| nome | descricao |\n|---|---|\n| a | {FRASE_LONGA} |\n");
        let lines = rendered_text_at(&md, 40);
        let junto = lines.join("\n");
        assert!(!junto.contains('…'), "não deve truncar: {junto}");
        for palavra in FRASE_LONGA.split(' ') {
            assert!(junto.contains(palavra), "faltou {palavra:?}: {junto}");
        }
        // borda, cabeçalho, separador, 3+ linhas de dado, borda
        assert!(lines.len() >= 7, "{lines:?}");
        assert_fits(&md, 40);
    }

    #[test]
    fn palavra_maior_que_a_coluna_quebra_por_caractere() {
        let md = tabela_de_uma_celula(&"x".repeat(100), "---");
        let junto = rendered_text_at(&md, 30).join("");
        assert_eq!(junto.matches('x').count(), 100, "{junto}");
        assert_fits(&md, 30);
    }

    #[test]
    fn teto_de_linhas_termina_com_reticencias() {
        let md = tabela_de_uma_celula(&"palavra ".repeat(200), "---");
        let lines = rendered_text_at(&md, 30);
        let dados: Vec<&String> = lines.iter().filter(|l| l.contains("palavra") || l.contains('…')).collect();
        assert_eq!(dados.len(), MAX_CELL_LINES, "{lines:?}");
        assert!(dados.last().unwrap().contains('…'), "{dados:?}");
        assert!(dados[..MAX_CELL_LINES - 1].iter().all(|l| !l.contains('…')), "{dados:?}");
    }

    #[test]
    fn celula_enorme_respeita_o_teto() {
        let md = tabela_de_uma_celula(&"x".repeat(500), "---");
        let lines = rendered_text_at(&md, 30);
        let dados: Vec<&String> = lines.iter().filter(|l| l.contains('x')).collect();
        assert_eq!(dados.len(), MAX_CELL_LINES, "{lines:?}");
        assert!(dados.last().unwrap().contains('…'), "{dados:?}");
    }

    #[test]
    fn alinhamento_a_direita_vale_em_cada_linha_da_celula() {
        let md = tabela_de_uma_celula("1234567890 1234567890", "--:");
        let lines = rendered_text_at(&md, 20);
        let dados: Vec<&String> = lines.iter().filter(|l| l.contains("1234567890")).collect();
        assert_eq!(dados.len(), 2, "{lines:?}");
        assert!(dados.iter().all(|l| l.ends_with("1234567890 │")), "{dados:?}");
    }

    #[test]
    fn todas_as_linhas_da_tabela_tem_a_mesma_largura() {
        let md = format!(
            "| a | b | c |\n|:--|:-:|--:|\n| {FRASE_LONGA} | 世界世界世界世界 | |\n| x | y | z |\n"
        );
        let larguras: std::collections::HashSet<usize> =
            render(&md, 40).lines.iter().filter(|l| !l.spans.is_empty()).map(Line::width).collect();
        assert_eq!(larguras.len(), 1, "{larguras:?}");
    }

    #[test]
    fn celula_com_estilo_mantem_o_estilo_apos_quebrar() {
        let md = tabela_de_uma_celula("**negrito comprido que quebra** fim", "---");
        let out = render(&md, 20);
        let negrito = out
            .lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .filter(|s| s.style.add_modifier.contains(Modifier::BOLD) && s.content.contains("negrito"))
            .count();
        assert!(negrito >= 1);
    }

    #[test]
    fn tabela_so_de_dados_quebra_celula_longa() {
        let rows = vec![vec!["id".to_string(), FRASE_LONGA.to_string()]];
        let out = render_table_only(rows, false, vec![Alignment::Left; 2], 30);
        let junto: String = out.lines.iter().map(line_to_string).collect::<Vec<_>>().join("\n");
        assert!(junto.contains("celula") && !junto.contains('…'), "{junto}");
    }

#[test]
fn tabela_com_colunas_ocultas_avisa() {
    let n = 30;
    let header = format!("|{}\n", " h |".repeat(n));
    let sep = format!("|{}\n", "---|".repeat(n));
    let lines = rendered_text_at(&format!("{header}{sep}"), 20);
    assert!(lines.iter().any(|l| l.contains("colunas")), "{lines:?}");
}

#[test]
fn caractere_largo_nao_estoura_uma_coluna() {
    let pieces = vec![("世界a".to_string(), Style::default())];
    for chunk in split_by_width(&pieces, 1) {
        let w: usize = chunk.iter().map(|(t, _)| t.width()).sum();
        assert!(w <= 1, "chunk com {w} colunas: {chunk:?}");
    }
}

#[test]
fn lista_dentro_de_item_mantem_marcador_externo() {
    let lines = rendered_text("- - x\n");
    assert_eq!(lines[0], "• ◦ x");
    let lines = rendered_text("1. - x\n   - y\n");
    assert!(lines[0].starts_with("1. ◦ x"), "{lines:?}");
    assert!(lines[1].starts_with("   ◦ y"), "{lines:?}");
}

#[test]
fn linhas_respeitam_largura_em_documento_misto() {
    let md = "> - item com texto comprido que precisa quebrar bastante\n>   ```\n>   codigo largo largo largo largo largo\n>   ```\n\n1. - # t\n";
    for w in [20, 25, 40, 80] {
        assert_fits(md, w);
    }
}

#[test]
fn entidade_de_nova_linha_nao_vira_quebra() {
    let lines = rendered_text("a&#10;b\n");
    assert_eq!(lines, vec!["a b"]);
}

#[test]
fn quebra_forcada_continua_quebrando_linha() {
    let lines = rendered_text("a\\\nb\n");
    assert_eq!(lines, vec!["a", "b"]);
}

#[test]
fn codigo_em_linha_preserva_espacos() {
    assert_eq!(rendered_text("x `a      b` y\n"), vec!["x a      b y"]);
}

#[test]
fn codigo_em_linha_longo_respeita_largura() {
    assert_fits("`um trecho de codigo com varias palavras separadas por espacos` fim\n", 20);
}

#[test]
fn espaco_sem_quebra_nao_e_separador_de_palavra() {
    assert_eq!(rendered_text("aaaaaaaaaa\u{a0}bbbbbbbbb\n"), vec!["aaaaaaaaaa\u{a0}bbbbbbbbb"]);
}

fn distinct_code_colors(code: &str) -> usize {
    let md = format!("```rust\n{code}```\n");
    let mut colors: Vec<Option<Color>> = Vec::new();
    for span in render(&md, 80).lines.iter().flat_map(|l| &l.spans) {
        if span.style.bg == Some(CODE_BG) && span.style.fg.is_some() && !colors.contains(&span.style.fg) {
            colors.push(span.style.fg);
        }
    }
    colors.len()
}

#[test]
fn codigo_pequeno_e_destacado() {
    assert!(distinct_code_colors("fn main() { let a = 1; } // x\n") > 1);
}

#[test]
fn codigo_enorme_nao_e_destacado_para_nao_travar_a_ui() {
    let line = "fn main() { let a = 1; } // x\n";
    let code = line.repeat(MAX_HIGHLIGHT_BYTES / line.len() + 1);
    assert_eq!(distinct_code_colors(&code), 1);
}

#[test]
fn orcamento_de_realce_vale_para_o_documento_inteiro() {
    let block = "fn main() { let a = 1; } // x\n".repeat(2000);
    let blocks = MAX_HIGHLIGHT_BYTES / block.len() + 2;
    let md = format!("```rust\n{block}```\n\n").repeat(blocks);
    let mut colors_per_block: Vec<std::collections::HashSet<String>> = Vec::new();
    for line in &render(&md, 80).lines {
        if line.spans.first().is_some_and(|s| s.content.starts_with('╭')) {
            colors_per_block.push(std::collections::HashSet::new());
        }
        for span in line.spans.iter().filter(|s| s.style.bg == Some(CODE_BG)) {
            if let (Some(set), Some(fg)) = (colors_per_block.last_mut(), span.style.fg) {
                set.insert(format!("{fg:?}"));
            }
        }
    }
    assert_eq!(colors_per_block.len(), blocks);
    assert!(colors_per_block[0].len() > 1, "primeiro bloco cabe no orçamento");
    assert_eq!(colors_per_block[blocks - 1].len(), 1, "último bloco estoura o orçamento");
}

#[test]
fn imagem_data_uri_vira_meio_bloco() {
    let img = image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(4, 4, image::Rgba([200, 10, 10, 255])));
    let mut png = std::io::Cursor::new(Vec::new());
    img.write_to(&mut png, image::ImageFormat::Png).unwrap();
    let uri = format!("data:image/png;base64,{}", crate::clip::base64(&png.into_inner()));
    let store = ImageStore::new(None);
    let r = render_with(&format!("![x]({uri})"), &RenderOpts { width: 40, images: Some(&store), ..Default::default() });
    assert!(joined_text(&r).contains('▀'));
}

#[test]
fn imagem_local_vira_meio_bloco() {
    let dir = tempfile::tempdir().unwrap();
    let img = image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(4, 4, image::Rgba([200, 10, 10, 255])));
    let mut png = std::io::Cursor::new(Vec::new());
    img.write_to(&mut png, image::ImageFormat::Png).unwrap();
    std::fs::write(dir.path().join("a.png"), png.into_inner()).unwrap();
    let store = ImageStore::new(Some(dir.path().to_path_buf()));
    let r = render_with("antes\n\n![logo](a.png)\n\ndepois", &RenderOpts { width: 40, images: Some(&store), ..Default::default() });
    let t = joined_text(&r);
    assert!(t.contains('▀'), "{t}");
    assert!(!t.contains("[imagem"), "{t}");
    assert!(t.contains("antes") && t.contains("depois"));
}

#[test]
fn imagem_quebrada_mostra_alt_e_motivo() {
    let dir = tempfile::tempdir().unwrap();
    let store = ImageStore::new(Some(dir.path().to_path_buf()));
    let r = render_with("![logo](nao.png)", &RenderOpts { width: 40, images: Some(&store), ..Default::default() });
    let t = joined_text(&r);
    assert!(t.contains("[imagem: logo"), "{t}");
    assert!(t.contains('—'), "{t}");
}

#[test]
fn imagem_com_graficos_vira_placeholders_e_registra_o_png() {
    let dir = tempfile::tempdir().unwrap();
    png_no_dir(dir.path(), "a.png"); // 4×4 px
    let store = ImageStore::new(Some(dir.path().to_path_buf())).with_graphics(true);
    let r = render_with("![x](a.png)", &RenderOpts { width: 40, images: Some(&store), ..Default::default() });
    let t = joined_text(&r);
    assert!(t.contains('\u{10EEEE}') && !t.contains('▀'), "{t:?}");
    assert_eq!(r.graphics.len(), 1);
    let g = &r.graphics[0];
    assert_eq!((g.cols, g.rows), (4, 2));
    assert_eq!(r.lines.len(), g.rows);
    assert_eq!(&g.png[..4], b"\x89PNG");
}

#[test]
fn mesma_imagem_duas_vezes_registra_um_png_so() {
    let dir = tempfile::tempdir().unwrap();
    png_no_dir(dir.path(), "a.png");
    let store = ImageStore::new(Some(dir.path().to_path_buf())).with_graphics(true);
    let r = render_with("![a](a.png)\n\n![b](a.png)", &RenderOpts { width: 40, images: Some(&store), ..Default::default() });
    assert_eq!(r.graphics.len(), 1);
    assert!(r.lines.iter().filter(|l| l.spans.iter().any(|s| s.content.contains('\u{10EEEE}'))).count() >= 4);
}

#[test]
fn sem_graficos_continua_meio_bloco_e_sem_registro() {
    let dir = tempfile::tempdir().unwrap();
    png_no_dir(dir.path(), "a.png");
    let store = ImageStore::new(Some(dir.path().to_path_buf()));
    let r = render_with("![x](a.png)", &RenderOpts { width: 40, images: Some(&store), ..Default::default() });
    assert!(joined_text(&r).contains('▀'));
    assert!(r.graphics.is_empty());
}

#[test]
fn imagem_em_lista_com_graficos_mantem_o_prefixo() {
    let dir = tempfile::tempdir().unwrap();
    png_no_dir(dir.path(), "a.png");
    let store = ImageStore::new(Some(dir.path().to_path_buf())).with_graphics(true);
    let r = render_with("- item\n  ![x](a.png)\n", &RenderOpts { width: 40, images: Some(&store), ..Default::default() });
    let img_line = r.lines.iter().find(|l| l.spans.iter().any(|s| s.content.contains('\u{10EEEE}'))).unwrap();
    assert!(img_line.spans.len() >= 2, "o recuo da lista vem antes dos placeholders");
}

#[test]
fn imagem_em_celula_vira_referencia_e_aparece_depois_da_tabela() {
    let dir = tempfile::tempdir().unwrap();
    png_no_dir(dir.path(), "a.png");
    let store = ImageStore::new(Some(dir.path().to_path_buf()));
    let r = render_with("| a |\n|---|\n| ![x](a.png) |\n\ndepois", &RenderOpts { width: 40, images: Some(&store), ..Default::default() });
    let t = joined_text(&r);
    assert!(t.contains("[imagem: x ↓1]"), "{t}");
    assert!(t.contains("↓1 x"), "{t}");
    assert!(t.contains('▀'), "{t}");
    assert!(t.find("[imagem: x ↓1]") < t.find('▀'), "a figura vem depois da tabela\n{t}");
    assert!(t.find('▀') < t.find("depois"), "{t}");
}

#[test]
fn varias_imagens_na_tabela_sao_numeradas_na_ordem() {
    let dir = tempfile::tempdir().unwrap();
    png_no_dir(dir.path(), "a.png");
    png_no_dir(dir.path(), "b.png");
    let store = ImageStore::new(Some(dir.path().to_path_buf()));
    let md = "| c1 | c2 |\n|---|---|\n| ![um](a.png) | ![dois](b.png) |\n";
    let t = joined_text(&render_with(md, &RenderOpts { width: 60, images: Some(&store), ..Default::default() }));
    assert!(t.contains("↓1 um") && t.contains("↓2 dois"), "{t}");
    assert!(t.find("↓1 um") < t.find("↓2 dois"), "{t}");
}

#[test]
fn imagem_quebrada_na_celula_mostra_o_motivo() {
    let dir = tempfile::tempdir().unwrap();
    let store = ImageStore::new(Some(dir.path().to_path_buf()));
    let t = joined_text(&render_with("| a |\n|---|\n| ![x](nao.png) |\n", &RenderOpts { width: 60, images: Some(&store), ..Default::default() }));
    assert!(t.contains("[imagem: x") && t.contains('—'), "{t}");
    assert!(!t.contains("↓1"), "{t}");
}

#[test]
fn legenda_longa_da_figura_nao_passa_da_largura() {
    let dir = tempfile::tempdir().unwrap();
    png_no_dir(dir.path(), "a.png");
    let store = ImageStore::new(Some(dir.path().to_path_buf()));
    let alt = "palavra ".repeat(30);
    let md = format!("| a |\n|---|\n| ![{alt}](a.png) |\n");
    let r = render_with(&md, &RenderOpts { width: 30, images: Some(&store), ..Default::default() });
    for l in &r.lines {
        let w: usize = l.spans.iter().map(|s| s.content.width()).sum();
        assert!(w <= 30, "linha com {w} colunas");
    }
}

#[test]
fn bloco_mermaid_vira_imagem_quando_habilitado() {
    if !crate::mermaid_native::fonts_available() {
        return; // máquina sem fontes: o nativo não desenha texto
    }
    let store = ImageStore::new(None).with_native_mermaid(true);
    let r = render_with(
        "```mermaid\nflowchart LR\n  A[Início] --> B[Fim]\n```",
        &RenderOpts { width: 40, images: Some(&store), ..Default::default() },
    );
    assert!(joined_text(&r).contains('▀'));
}

#[test]
fn mermaid_sem_flag_mostra_codigo() {
    let t = joined_text(&render("```mermaid\ngraph TD; A-->B\n```", 40));
    assert!(t.contains("mermaid") && t.contains("A-->B"), "{t}");
}

#[test]
fn mermaid_com_falha_mostra_codigo_e_motivo() {
    let store = ImageStore::new(None).with_native_mermaid(true);
    let r = render_with("```mermaid\nisto não é mermaid\n```", &RenderOpts { width: 60, images: Some(&store), ..Default::default() });
    let t = joined_text(&r);
    assert!(t.contains("isto não é mermaid") && t.contains("erro"), "{t}");
}

#[test]
fn codigo_sem_quebra_mantem_uma_linha_por_linha_do_fonte() {
    let md = format!("```\n{}\n```", "x".repeat(100));
    let r = render_with(&md, &RenderOpts { width: 40, code_scroll: Some(0), ..Default::default() });
    let t = joined_text(&r);
    assert_eq!(r.lines.len(), 3, "{t}"); // cabeçalho, 1 linha, rodapé
    assert!(t.contains('›') && !t.contains('‹'), "{t}");
}

#[test]
fn codigo_com_deslocamento_mostra_o_trecho_e_as_duas_bordas() {
    // largura 20 => 18 colunas de código
    let md = "```\n0123456789abcdefghijklmnopqrstuvwxyz0123456789\n```";
    let r = render_with(md, &RenderOpts { width: 20, code_scroll: Some(10), ..Default::default() });
    let t = joined_text(&r);
    assert!(t.contains("‹bcdefghijklmnopq›"), "{t}");
    assert_eq!(r.code_overflow, 46 - 18);
}

#[test]
fn codigo_curto_nao_ganha_marcas_nem_overflow() {
    let r = render_with("```\nab\n```", &RenderOpts { width: 20, code_scroll: Some(0), ..Default::default() });
    let t = joined_text(&r);
    assert!(!t.contains('›') && !t.contains('‹'), "{t}");
    assert_eq!(r.code_overflow, 0);
}

#[test]
fn codigo_sem_opcao_continua_quebrando() {
    let md = format!("```\n{}\n```", "x".repeat(100));
    let r = render_with(&md, &RenderOpts { width: 40, ..Default::default() });
    assert!(r.lines.len() > 3);
}

#[test]
fn janela_de_codigo_com_caractere_largo_na_borda_nao_estoura() {
    // "日" ocupa 2 colunas; a janela de 18 colunas nunca pode passar disso
    let md = format!("```\n{}\n```", "日".repeat(30));
    for skip in [0, 1, 5] {
        let r = render_with(&md, &RenderOpts { width: 20, code_scroll: Some(skip), ..Default::default() });
        for l in &r.lines {
            let w: usize = l.spans.iter().map(|s| s.content.width()).sum();
            assert!(w <= 20, "skip {skip}: linha com {w} colunas");
        }
    }
}

#[test]
fn fonte_crua_tambem_aceita_deslocamento() {
    let r = render_code_only_at(&"y".repeat(100), "", 30, Some(0));
    assert_eq!(r.lines.len(), 3);
}

#[test]
fn front_matter_vira_bloco_yaml() {
    let t = t60("---\ntitle: Plano\ntags: [a]\n---\n\n# Título");
    assert!(t.contains("yaml") && t.contains("title: Plano"), "{t}");
    assert!(!t.contains("---"), "{t}");
}

#[test]
fn alerta_gfm_mostra_rotulo_em_portugues() {
    let t = t60("> [!WARNING]\n> cuidado aqui");
    assert!(t.contains("Aviso") && t.contains("cuidado aqui"), "{t}");
    assert!(!t.contains("[!WARNING]"), "{t}");
}

#[test]
fn nota_de_rodape_aparece_no_texto_e_na_definicao() {
    let t = t60("Fato[^1].\n\n[^1]: Fonte aqui.");
    assert!(t.contains("[^1]") && t.contains("Fonte aqui"), "{t}");
}

#[test]
fn details_e_summary_viram_titulo() {
    let t = t60("<details>\n<summary>Ver mais</summary>\n\nconteúdo\n\n</details>");
    assert!(t.contains("▸ Ver mais") && t.contains("conteúdo"), "{t}");
    assert!(!t.contains("<details>") && !t.contains("<summary>"), "{t}");
}

#[test]
fn think_e_html_comum_sem_rotulo_nem_estilo_proprio() {
    let t = t60("<think>\n\nplano interno\n\n</think>\n\nresposta final");
    assert!(t.contains("<think>") && t.contains("plano interno") && t.contains("</think>"), "{t}");
    assert!(!t.contains("raciocínio"), "{t}");
}

#[test]
fn cerca_sem_fechar_nao_quebra() {
    let t = joined_text(&render("```rust\nfn main() {", 40));
    assert!(t.contains("fn main"), "{t}");
}

#[test]
fn fonte_crua_mostra_markdown_literal() {
    let t = joined_text(&render_code_only("# a\n**b**\n", "md", 40));
    assert!(t.contains("# a") && t.contains("**b**") && t.contains("md"), "{t}");
}

fn todas(r: &Rendered) -> TaskCount {
    count_tasks(&r.tasks, 0, usize::MAX)
}

#[test]
fn conta_tarefas_em_lista_aninhada() {
    let r = render("- [x] a\n- [ ] b\n  - [x] c\n", 40);
    assert_eq!(todas(&r), TaskCount { done: 2, total: 3 });
}

#[test]
fn conta_tarefa_marcada_dentro_de_citacao() {
    let r = render("> - [x] q\n> - [ ] w\n", 40);
    assert_eq!(todas(&r), TaskCount { done: 1, total: 2 });
}

#[test]
fn nao_conta_colchetes_em_codigo_nem_em_texto_corrido() {
    let r = render("```\n- [x] falso\n```\n\ntexto [ ] solto\n\n[x] sem lista\n", 40);
    assert_eq!(todas(&r), TaskCount::default());
}

#[test]
fn count_tasks_usa_intervalo_fechado_aberto() {
    let tasks = [TaskItem { line: 2, done: true }, TaskItem { line: 5, done: false }];
    assert_eq!(count_tasks(&tasks, 2, 5), TaskCount { done: 1, total: 1 });
    assert_eq!(count_tasks(&tasks, 2, 6), TaskCount { done: 1, total: 2 });
    assert_eq!(count_tasks(&tasks, 3, 5), TaskCount::default());
}

#[test]
fn secao_sem_tarefas_conta_zero() {
    let r = render("# A\n\n- [x] a\n\n# B\n\ntexto\n", 40);
    assert_eq!(section_tasks(&r, 0), TaskCount { done: 1, total: 1 });
    assert_eq!(section_tasks(&r, 1), TaskCount::default());
}

#[test]
fn contagem_do_titulo_inclui_as_subsecoes() {
    let r = render("# A\n\n- [x] a\n\n## B\n\n- [ ] b\n\n## C\n\n- [x] c\n", 40);
    assert_eq!(section_tasks(&r, 0), TaskCount { done: 2, total: 3 });
    assert_eq!(section_tasks(&r, 1), TaskCount { done: 0, total: 1 });
    assert_eq!(section_tasks(&r, 2), TaskCount { done: 1, total: 1 });
}

#[test]
fn tarefa_antes_do_primeiro_titulo_conta_so_no_documento() {
    let r = render("- [x] solta\n\n# A\n\ntexto\n", 40);
    assert_eq!(todas(&r), TaskCount { done: 1, total: 1 });
    assert_eq!(section_tasks(&r, 0), TaskCount::default());
}

#[test]
fn toc_acrescenta_a_contagem_aos_titulos_com_tarefas() {
    let r = render("# A\n\n- [x] a\n\n## B\n\n- [ ] b\n\n# C\n\ntexto\n", 40);
    assert_eq!(toc(&r), "A [1/2]\n  B [0/1]\nC\n");
}

#[test]
fn toc_indenta_por_nivel() {
    let r = render("# A\n\n## B\n\n### C\n\n## D", 40);
    assert_eq!(toc(&r), "A\n  B\n    C\n  D\n");
}

#[test]
fn toc_sem_titulos_e_vazio() {
    assert_eq!(toc(&render("só texto", 40)), "");
}

#[test]
fn toc_usa_texto_sem_formatacao_e_posicao_da_linha() {
    let r = render("texto\n\n## Um **forte** título", 40);
    assert_eq!(toc(&r), "  Um forte título\n");
    assert_eq!(r.outline[0].line, r.headings[0]);
}

#[test]
fn erro_renderizado_tem_uma_linha_e_sanitiza() {
    assert_eq!(error_rendered("falhou").lines.len(), 1);
    assert!(!joined_text(&error_rendered("a\x1b[31mb")).contains('\x1b'));
}

#[test]
fn escape_em_rotulo_de_nota_de_rodape_nao_vaza() {
    let r = render("Texto[^a\x1bcb].\n\n[^a\x1bcb]: nota\n", 60);
    assert!(!to_ansi(&r.lines, false).contains('\x1b'));
}

#[test]
fn escape_em_motivo_de_falha_de_imagem_nao_vaza() {
    let url = "https://exemplo.invalido/k.png";
    let store = ImageStore::new(None).with_remote(crate::net::FetchPolicy::default(), true);
    store.get_job(url, false, || Err("falha\x1bc\x07".to_string()));
    let r = render_with(&format!("![x]({url})"), &RenderOpts { width: 60, images: Some(&store), ..Default::default() });
    let ansi = to_ansi(&r.lines, false);
    assert!(ansi.contains("falha") && !ansi.contains('\x1b') && !ansi.contains('\x07'), "{ansi:?}");
}

#[test]
fn sem_store_mantem_comportamento_antigo() {
    let t = joined_text(&render("![logo](a.png)", 40));
    assert_eq!(t.trim(), "[imagem: logo]");
}

    fn tabela_de_n_colunas(n: usize) -> String {
        let header: String = (1..=n).map(|i| format!(" c{i} |")).collect();
        let sep = "---|".repeat(n);
        let row: String = (1..=n).map(|i| format!(" v{i} |")).collect();
        format!("|{header}\n|{sep}\n|{row}\n")
    }

    #[test]
    fn render_registra_o_intervalo_de_cada_tabela() {
        let md = "antes\n\n| x | y |\n|---|---|\n| 1 | 2 |\n\ndepois\n";
        let out = render(md, 40);
        assert_eq!(out.tables.len(), 1);
        let t = &out.tables[0];
        assert!(line_to_string(&out.lines[t.start]).starts_with('┌'));
        assert!(line_to_string(&out.lines[t.end - 1]).starts_with('└'));
        assert_eq!(t.data.total_cols(), 2);
    }

    #[test]
    fn tabela_em_citacao_e_lista_registra_o_intervalo_com_recuo() {
        let out = render("> | x |\n> |---|\n> | 1 |\n", 40);
        assert_eq!(out.tables.len(), 1);
        let t = &out.tables[0];
        assert!(line_to_string(&out.lines[t.start]).contains('┌'));
        let out = render("- item\n\n  | x |\n  |---|\n  | 1 |\n", 40);
        assert_eq!(out.tables.len(), 1);
        assert!(out.tables[0].end <= out.lines.len());
    }

    #[test]
    fn duas_tabelas_ficam_em_ordem_e_sem_sobreposicao() {
        let md = "| a |\n|---|\n| 1 |\n\ntexto\n\n| b |\n|---|\n| 2 |\n";
        let out = render(md, 40);
        assert_eq!(out.tables.len(), 2);
        assert!(out.tables[0].end <= out.tables[1].start);
    }

    #[test]
    fn bloco_diff_pinta_adicoes_e_remocoes() {
        let r = render_with("```diff\n-a\n+b\n c\n```\n", &RenderOpts { width: 40, images: None, ..Default::default() });
        let bg = |i: usize| r.lines[i].spans.iter().find_map(|s| s.style.bg);
        assert_eq!(bg(1), Some(DIFF_DEL_BG));
        assert_eq!(bg(2), Some(DIFF_ADD_BG));
        assert_eq!(bg(3), Some(CODE_BG));
    }

    #[test]
    fn render_table_only_registra_a_tabela() {
        let out = render_table_only(vec![vec!["a".into(), "b".into()]], false, vec![Alignment::Left; 2], 40);
        assert_eq!(out.tables.len(), 1);
        assert_eq!((out.tables[0].start, out.tables[0].end), (0, out.lines.len()));
    }

    #[test]
    fn draw_nao_tem_recuo_e_fixa_cabecalho() {
        let out = render("> | x | y |\n> |---|---|\n> | 1 | 2 |\n", 40);
        let v = out.tables[0].data.draw(40, 0);
        assert!(line_to_string(&v.lines[0]).starts_with('┌'), "sem o '▎ ' da citação");
        assert_eq!(v.pinned, 3, "borda, cabeçalho e separador");
        assert_eq!((v.first_col, v.shown_cols, v.total_cols), (1, 2, 2));
    }

    #[test]
    fn draw_sem_cabecalho_fixa_so_a_borda() {
        let out = render_table_only(vec![vec!["a".into()]], false, vec![Alignment::Left], 40);
        assert_eq!(out.tables[0].data.draw(40, 0).pinned, 1);
    }

    #[test]
    fn draw_com_uma_coluna_nao_estoura_o_indice() {
        let out = render("| só |\n|---|\n| 1 |\n", 40);
        let v = out.tables[0].data.draw(20, usize::MAX);
        assert_eq!((v.first_col, v.shown_cols, v.total_cols), (0, 1, 1));
    }

    #[test]
    fn draw_rola_por_coluna_e_mantem_a_primeira() {
        let out = render(&tabela_de_n_colunas(12), 20);
        let data = &out.tables[0].data;

        let v = data.draw(20, 0);
        assert_eq!((v.first_col, v.shown_cols), (1, 3));
        let cab = line_to_string(&v.lines[1]);
        assert!(cab.contains("│ c1 ") && cab.contains("c2"), "{cab}");
        assert!(line_to_string(v.lines.last().unwrap()).contains("+9 colunas"));

        let v = data.draw(20, 5);
        assert_eq!(v.first_col, 5);
        let cab = line_to_string(&v.lines[1]);
        assert!(cab.contains("│ c1 ") && cab.contains("c6") && !cab.contains("c2 "), "{cab}");

        let v = data.draw(20, usize::MAX);
        assert_eq!(v.first_col + v.shown_cols - 1, v.total_cols, "última coluna à vista");
        assert!(line_to_string(&v.lines[1]).contains("c12"));
    }

    #[test]
    fn draw_cabe_em_qualquer_largura_pedida() {
        let out = render(&tabela_de_n_colunas(30), 80);
        for w in [20, 33, 80, 200] {
            for first in [0, 3, usize::MAX] {
                let v = out.tables[0].data.draw(w, first);
                assert!(v.lines.iter().all(|l| l.width() <= w), "largura {w}, coluna {first}");
            }
        }
    }

fn linha_de(r: &Rendered, i: usize) -> String {
    r.lines[i].spans.iter().map(|s| s.content.as_ref()).collect()
}

fn indice_da_linha_com(r: &Rendered, trecho: &str) -> usize {
    (0..r.lines.len())
        .find(|&i| linha_de(r, i).contains(trecho))
        .unwrap_or_else(|| panic!("nenhuma linha contém {trecho:?}"))
}

#[test]
fn line_src_tem_uma_entrada_por_linha_renderizada() {
    let md = "# T\n\npar um\n\n- a\n- b\n\n```rust\nfn x() {}\n```\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\n> citação\n";
    let r = render(md, 40);
    assert_eq!(r.line_src.len(), r.lines.len());
}

#[test]
fn line_src_aponta_para_a_linha_do_fonte() {
    let r = render("# Título\n\nprimeiro\n\nsegundo parágrafo\n", 40);
    assert_eq!(r.line_src[indice_da_linha_com(&r, "Título")], 0);
    assert_eq!(r.line_src[indice_da_linha_com(&r, "primeiro")], 2);
    assert_eq!(r.line_src[indice_da_linha_com(&r, "segundo")], 4);
}

#[test]
fn line_src_de_lista_aninhada_e_citacao_usa_a_linha_de_cada_item() {
    let r = render("- a\n  - b\n- c\n\n> q1\n> q2\n", 40);
    assert_eq!(r.line_src[indice_da_linha_com(&r, "a")], 0);
    assert_eq!(r.line_src[indice_da_linha_com(&r, "b")], 1);
    assert_eq!(r.line_src[indice_da_linha_com(&r, "c")], 2);
    assert_eq!(r.line_src[indice_da_linha_com(&r, "q1")], 4);
}

#[test]
fn linha_quebrada_pela_largura_herda_a_linha_do_bloco() {
    let md = format!("# T\n\n{}\n\nfim\n", "palavra ".repeat(30));
    let r = render(&md, 30);
    let do_bloco: Vec<usize> = (0..r.lines.len())
        .filter(|&i| linha_de(&r, i).contains("palavra"))
        .map(|i| r.line_src[i])
        .collect();
    assert!(do_bloco.len() > 1, "o parágrafo deveria quebrar em várias linhas");
    assert!(do_bloco.iter().all(|&s| s == 2), "{do_bloco:?}");
}

#[test]
fn bloco_de_codigo_registra_intervalo_linguagem_e_texto_cru() {
    let r = render("x\n\n```rust\nfn a() {}\n```\n\n~~~\nsem lang\n~~~\n", 40);
    assert_eq!(r.code_blocks.len(), 2);
    let (a, b) = (&r.code_blocks[0], &r.code_blocks[1]);
    assert_eq!(a.lang, "rust");
    assert_eq!(a.text, "fn a() {}\n");
    assert!(linha_de(&r, a.start).starts_with("╭─"));
    assert!(linha_de(&r, a.end - 1).starts_with("╰─"));
    assert_eq!(b.lang, "");
    assert_eq!(b.text, "sem lang\n");
}

#[test]
fn tarefas_registram_feita_ou_aberta_na_linha_do_item() {
    let r = render("# T\n\n- [x] a\n- [ ] b\n  - [x] c\n\n> - [ ] d\n", 40);
    let feitas: Vec<bool> = r.tasks.iter().map(|t| t.done).collect();
    assert_eq!(feitas, vec![true, false, true, false]);
    for t in &r.tasks {
        let marca = if t.done { '☑' } else { '☐' };
        assert!(linha_de(&r, t.line).contains(marca), "linha {}: {:?}", t.line, linha_de(&r, t.line));
    }
}

#[test]
fn section_range_vai_ate_o_proximo_titulo_de_nivel_igual_ou_menor() {
    let r = render("# A\n\ntexto\n\n## B\n\nb\n\n## C\n\nc\n\n# D\n\nd\n", 40);
    let o = &r.outline;
    assert_eq!(section_range(o, 0, r.lines.len()), (o[0].line, o[3].line));
    assert_eq!(section_range(o, 1, r.lines.len()), (o[1].line, o[2].line));
    assert_eq!(section_range(o, 3, r.lines.len()), (o[3].line, r.lines.len()));
}

#[test]
fn section_at_acha_o_titulo_mais_profundo_antes_da_linha() {
    let r = render("antes\n\n# A\n\ntexto\n\n## B\n\nb\n", 40);
    let o = &r.outline;
    assert_eq!(section_at(o, 0), None, "antes do primeiro título");
    assert_eq!(section_at(o, o[0].line), Some(0));
    assert_eq!(section_at(o, indice_da_linha_com(&r, "texto")), Some(0));
    assert_eq!(section_at(o, indice_da_linha_com(&r, "b")), Some(1));
}

#[test]
fn section_source_devolve_o_markdown_cru_com_o_titulo() {
    let md = "intro\n\n# A\n\ntexto\n\n## B\n\nb\n\n# C\n\nc\n";
    let r = render(md, 40);
    assert_eq!(section_source(md, &r, 0).as_deref(), Some("# A\n\ntexto\n\n## B\n\nb\n"));
    assert_eq!(section_source(md, &r, 1).as_deref(), Some("## B\n\nb\n"));
    assert_eq!(section_source(md, &r, 2).as_deref(), Some("# C\n\nc\n"), "última seção vai ao fim");
    assert_eq!(section_source(md, &r, 9), None);
}

#[test]
fn section_source_sem_mapa_de_linhas_devolve_none() {
    let r = error_rendered("x");
    assert_eq!(section_source("# A\n", &r, 0), None);
}

struct FakeImages {
    state: fn() -> ImageState,
    graphics: bool,
    mermaid: bool,
}

fn quadrada() -> ImageState {
    ImageState::Ready(Arc::new(image::DynamicImage::new_rgb8(4, 4)))
}

impl ImageProvider for FakeImages {
    fn graphics_enabled(&self) -> bool {
        self.graphics
    }
    fn graphic(&self, _: &Arc<image::DynamicImage>, _: usize, _: usize) -> Option<crate::image::Graphic> {
        None
    }
    fn mermaid_enabled(&self) -> bool {
        self.mermaid
    }
    fn image(&self, _: &str) -> ImageState {
        (self.state)()
    }
    fn mermaid(&self, _: &str) -> ImageState {
        (self.state)()
    }
    fn mermaid_key(&self, source: &str) -> String {
        format!("fake:{}", source.len())
    }
}

fn com_fake(md: &str, f: &FakeImages) -> Rendered {
    render_with(md, &RenderOpts { width: 40, images: Some(f), ..Default::default() })
}

#[test]
fn fake_imagem_pronta_vira_meio_bloco_sem_disco() {
    let f = FakeImages { state: quadrada, graphics: false, mermaid: false };
    let r = com_fake("![x](qualquer.png)", &f);
    assert!(r.lines.iter().any(|l| l.spans.iter().any(|s| s.content.contains('▀'))));
}

#[test]
fn fake_imagem_carregando_e_falha_mostram_aviso() {
    let f = FakeImages { state: || ImageState::Loading, graphics: false, mermaid: false };
    assert!(joined_text(&com_fake("![x](a.png)", &f)).contains("… carregando]"));
    let f = FakeImages { state: || ImageState::Failed("quebrou".into()), graphics: false, mermaid: false };
    assert!(joined_text(&com_fake("![x](a.png)", &f)).contains("— quebrou]"));
}

#[test]
fn fake_mermaid_desligado_mostra_codigo() {
    let f = FakeImages { state: quadrada, graphics: false, mermaid: false };
    let r = com_fake("```mermaid\ngraph TD; A-->B\n```", &f);
    assert!(joined_text(&r).contains("graph TD"));
    assert!(r.mermaids.is_empty());
}

#[test]
fn fake_mermaid_ligado_vira_figura_e_registra_o_span() {
    let f = FakeImages { state: quadrada, graphics: false, mermaid: true };
    let r = com_fake("```mermaid\ngraph TD; A-->B\n```", &f);
    assert_eq!(r.mermaids.len(), 1);
    assert_eq!(r.mermaids[0].key, format!("fake:{}", "graph TD; A-->B\n".len()));
    assert!(!joined_text(&r).contains("graph TD"));
}

#[test]
fn fake_mermaid_renderizando_e_com_erro_mostram_codigo_com_rotulo() {
    let f = FakeImages { state: || ImageState::Loading, graphics: false, mermaid: true };
    assert!(joined_text(&com_fake("```mermaid\ngraph TD; A-->B\n```", &f)).contains("mermaid (renderizando…)"));
    let f = FakeImages { state: || ImageState::Failed("x".into()), graphics: false, mermaid: true };
    assert!(joined_text(&com_fake("```mermaid\ngraph TD; A-->B\n```", &f)).contains("mermaid (erro: x)"));
}

#[test]
fn fake_imagem_em_celula_vira_referencia_numerada() {
    let f = FakeImages { state: quadrada, graphics: false, mermaid: false };
    let t = joined_text(&com_fake("| a |\n|---|\n| ![x](a.png) |\n", &f));
    assert!(t.contains("↓1"));
}

/// Spec 4: nenhum arquivo com mais de ~500 linhas de produção (antes de `#[cfg(test)]`).
#[test]
fn arquivos_de_producao_tem_ate_500_linhas() {
    let arquivos: &[(&str, &str)] = &[
        ("mod.rs", include_str!("mod.rs")),
        ("model.rs", include_str!("model.rs")),
        ("outline.rs", include_str!("outline.rs")),
        ("theme.rs", include_str!("theme.rs")),
        ("ansi.rs", include_str!("ansi.rs")),
        ("wrap.rs", include_str!("wrap.rs")),
        ("inline.rs", include_str!("inline.rs")),
        ("list.rs", include_str!("list.rs")),
        ("table.rs", include_str!("table.rs")),
        ("images.rs", include_str!("images.rs")),
        ("highlight.rs", include_str!("highlight.rs")),
        ("renderer.rs", include_str!("renderer.rs")),
    ];
    for (nome, src) in arquivos {
        let producao = src.lines().take_while(|l| !l.starts_with("#[cfg(test)]")).count();
        assert!(producao <= 500, "{nome} tem {producao} linhas de produção");
    }
}
