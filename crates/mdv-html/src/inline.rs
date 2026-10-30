//! Texto em linha: percurso iterativo dos filhos (sem recursão), `Face` com links, `<br>`, `<q>`,
//! `<sup>`/`<sub>`, pseudoelementos `::before`/`::after` e controles de formulário e mídia viram
//! texto (`[imagem: alt]`, `[ Ir ]`, `[x]`, barras).

use crate::color;
use crate::ctx::Ctx;
use crate::form;
use crate::media;
use crate::model::{Cell, Face, Rgb};
use crate::script::{self, Script};
use crate::selector::Pseudo;
use crate::style::{compute, compute_pseudo, Display, Style, Transform, WhiteSpace};
use crate::text::push_text;
use crate::ua;
use scraper::node::Node;
use scraper::ElementRef;

/// Filho de um elemento: texto já copiado ou elemento.
pub enum Item<'a> {
    Texto(String),
    Elem(ElementRef<'a>),
}

/// O que um elemento herda do pai no texto corrido: o link e o estilo de script (`sup`/`sub`).
#[derive(Clone, Copy, Debug, Default)]
pub struct Herdado {
    pub link: Option<u32>,
    pub script: Option<Script>,
}

/// Um nível do percurso: filhos ainda não vistos, estilo e fundo do elemento, link e script
/// herdados e células a emitir quando o nível fecha (espaços de bloco, de pílula e aspas).
pub struct Quadro<'a> {
    itens: std::vec::IntoIter<Item<'a>>,
    st: Style,
    bg: Option<Rgb>,
    link: Option<u32>,
    script: Option<Script>,
    fim: Vec<Cell>,
}

/// Filhos de `el` em ordem de documento (comentários e outros nós são descartados).
pub fn filhos(el: ElementRef<'_>) -> Vec<Item<'_>> {
    el.children()
        .filter_map(|n| match n.value() {
            Node::Text(t) => Some(Item::Texto(t.text.to_string())),
            Node::Element(_) => ElementRef::wrap(n).map(Item::Elem),
            _ => None,
        })
        .collect()
}

/// Verdadeiro se o elemento não aparece (e nem seus filhos).
pub fn oculto(cs: &Style) -> bool {
    cs.display == Display::None || cs.hidden || cs.faded || cs.fixed
}

/// Verdadeiro se o elemento entra no texto corrido (em vez de abrir um bloco). O `textarea` é
/// uma caixa de várias linhas e abre bloco próprio.
pub fn em_linha(nome: &str, cs: &Style) -> bool {
    (e_controle(nome) && nome != "textarea")
        || matches!(
            cs.display,
            Display::Inline | Display::InlineBlock | Display::Contents
        )
}

/// Fundo efetivo de um elemento: o seu, misturado com o do pai quando tem alfa.
pub fn fundo_efetivo(st: &Style, paint: Option<Rgb>) -> Option<Rgb> {
    match st.bg {
        Some(c) if c.alpha > 0.01 => Some(color::blend(c, paint)),
        _ => paint,
    }
}

/// Estilo de célula a partir do estilo calculado, do fundo efetivo e do link.
pub fn face(st: &Style, bg: Option<Rgb>, link: Option<u32>) -> Face {
    Face {
        fg: color::fix_contrast(st.fg, bg),
        bg,
        bold: st.bold,
        italic: st.italic,
        underline: st.underline,
        dim: st.dim,
        strike: st.strike,
        link,
    }
}

/// Uma célula com `ch` e estilo `f`.
pub fn celula(ch: char, f: Face) -> Cell {
    Cell { ch, face: f }
}

/// Células do conteúdo de `el` (sem a própria caixa), com espaços preservados se `st` for `pre`.
pub fn celulas_filhos(
    el: ElementRef<'_>,
    st: &Style,
    bg: Option<Rgb>,
    cx: &Ctx<'_>,
) -> Result<Vec<Cell>, String> {
    let mut out = Vec::new();
    let mut pilha = vec![Quadro {
        itens: filhos(el).into_iter(),
        st: st.clone(),
        bg,
        link: None,
        script: None,
        fim: Vec::new(),
    }];
    executa(&mut pilha, cx, &mut out)?;
    Ok(out)
}

/// Células de um elemento em linha (com seus filhos), acrescentadas em `out`.
pub fn elemento(
    c: ElementRef<'_>,
    cs: Style,
    pai: &Style,
    pai_bg: Option<Rgb>,
    cx: &Ctx<'_>,
    out: &mut Vec<Cell>,
) -> Result<(), String> {
    let mut pilha = Vec::new();
    if let Some(q) = abre(c, cs, pai, pai_bg, Herdado::default(), cx, out)? {
        pilha.push(q);
    }
    executa(&mut pilha, cx, out)
}

/// Percorre os níveis abertos até esvaziar a pilha. Cada elemento visitado passa por `tick`.
fn executa<'a>(
    pilha: &mut Vec<Quadro<'a>>,
    cx: &Ctx<'_>,
    out: &mut Vec<Cell>,
) -> Result<(), String> {
    while let Some(q) = pilha.last_mut() {
        let Some(item) = q.itens.next() else {
            if let Some(fechado) = pilha.pop() {
                out.extend(fechado.fim);
            }
            continue;
        };
        match item {
            Item::Texto(t) => {
                let t = match q.script {
                    Some(s) if s.mapear => script::converte(&t, s.sup),
                    _ => t,
                };
                push_text(out, &t, face(&q.st, q.bg, q.link), q.st.white, q.st.transform);
            }
            Item::Elem(c) => {
                cx.tick()?;
                let cs = compute(c, &q.st, cx.cascade);
                if oculto(&cs) {
                    continue;
                }
                if c.value().name() == "br" {
                    out.push(celula('\n', Face::default()));
                    continue;
                }
                let herdado = Herdado {
                    link: q.link,
                    script: q.script,
                };
                let novo = abre(c, cs, &q.st, q.bg, herdado, cx, out)?;
                if let Some(n) = novo {
                    pilha.push(n);
                }
            }
        }
    }
    Ok(())
}

/// Abre um elemento em linha: emite o que vem antes dele e devolve o nível dos filhos.
/// Controles de formulário e mídia são texto direto: devolvem `None`.
fn abre<'a>(
    c: ElementRef<'a>,
    cs: Style,
    pai: &Style,
    pai_bg: Option<Rgb>,
    herdado: Herdado,
    cx: &Ctx<'_>,
    out: &mut Vec<Cell>,
) -> Result<Option<Quadro<'a>>, String> {
    let link = match (c.value().name(), c.value().attr("href")) {
        ("a", Some(href)) => Some(cx.add_link(href)),
        _ => herdado.link,
    };
    let cbg = fundo_efetivo(&cs, pai_bg);
    if let Some(celulas) = controle(c, &cs, pai, cbg, link, cx)? {
        out.extend(celulas);
        return Ok(None);
    }
    let nome = c.value().name();
    let pre = matches!(pai.white, WhiteSpace::Pre | WhiteSpace::PreWrap);
    let bloco = !matches!(cs.display, Display::Inline | Display::Contents) && !pre;
    let pilula = !pre && cs.bg.is_some_and(|c| c.alpha > 0.01);
    let script = script_de(c, nome, herdado.script, cx)?;
    let mut fim = Vec::new();
    if bloco {
        out.push(celula(' ', face(pai, pai_bg, herdado.link)));
        fim.push(celula(' ', face(pai, pai_bg, herdado.link)));
    }
    // Pílula: espaço só com padding horizontal do autor (4 px = 1 coluna; 20 px = 2).
    if pilula {
        let (esq, dir) = espacos_de_pilula(c, &cs);
        let f = face(&cs, cbg, link);
        out.extend(espacos_pilula(esq, true).into_iter().map(|ch| celula(ch, f)));
        let mut com_dir: Vec<Cell> = espacos_pilula(dir, false)
            .into_iter()
            .map(|ch| celula(ch, f))
            .collect();
        com_dir.extend(fim);
        fim = com_dir;
    }
    if nome == "q" {
        let f = face(&cs, cbg, link);
        out.push(celula('“', f));
        fim.insert(0, celula('”', f));
    }
    if let Some(s) = script {
        if !s.mapear && matches!(nome, "sup" | "sub") {
            out.push(celula(script::prefixo(s.sup), face(&cs, cbg, link)));
        }
    }
    out.extend(pseudo_celulas(c, &cs, Pseudo::Before, cbg, link, cx));
    let mut depois = pseudo_celulas(c, &cs, Pseudo::After, cbg, link, cx);
    depois.extend(fim);
    Ok(Some(Quadro {
        itens: filhos(c).into_iter(),
        st: cs,
        bg: cbg,
        link,
        script,
        fim: depois,
    }))
}

/// Espaços de cada lado de uma pílula, pelo padding horizontal do autor. Não há espaço do lado em
/// que o texto vizinho é pontuação (`sk_test.`) ou o que abre um parêntese ou colchete (`(x`).
fn espacos_de_pilula(c: ElementRef<'_>, cs: &Style) -> (usize, usize) {
    let esq = espacos_de_padding(cs.padding[3]);
    let dir = espacos_de_padding(cs.padding[1]);
    let antes = c.prev_sibling().and_then(|n| match n.value() {
        Node::Text(t) => t.text.chars().last(),
        _ => None,
    });
    let depois = c.next_sibling().and_then(|n| match n.value() {
        Node::Text(t) => t.text.chars().next(),
        _ => None,
    });
    let esq = if antes.is_some_and(|ch| matches!(ch, '(' | '[' | '{' | '“' | '‘')) {
        0
    } else {
        esq
    };
    let dir = if depois.is_some_and(|ch| !ch.is_alphanumeric() && !ch.is_whitespace()) {
        0
    } else {
        dir
    };
    (esq, dir)
}

/// Espaços de uma pílula: o `wrap` colapsa espaços seguidos, então só o espaço da borda externa
/// é separável (`' '`); os demais são sem quebra (U+00A0). `esquerda` indica o lado do texto.
fn espacos_pilula(n: usize, esquerda: bool) -> Vec<char> {
    let fixos = "\u{a0}".repeat(n.saturating_sub(1));
    let (inicio, fim) = if esquerda {
        (" ", fixos.as_str())
    } else {
        (fixos.as_str(), " ")
    };
    if n == 0 {
        return Vec::new();
    }
    format!("{inicio}{fim}").chars().collect()
}

/// Um espaço a partir de 4 px de padding, dois a partir de 20 px, nenhum abaixo disso.
fn espacos_de_padding(px: f32) -> usize {
    if px >= 20.0 {
        2
    } else if px >= 4.0 {
        1
    } else {
        0
    }
}

/// Script de `c`: `sup` e `sub` decidem pelo texto do próprio elemento; os demais herdam.
fn script_de(
    c: ElementRef<'_>,
    nome: &str,
    herdado: Option<Script>,
    cx: &Ctx<'_>,
) -> Result<Option<Script>, String> {
    let sup = match nome {
        "sup" => true,
        "sub" => false,
        _ => return Ok(herdado),
    };
    let texto = script::texto_bruto(c, cx)?;
    Ok(Some(Script {
        sup,
        mapear: script::mapear(&texto, sup),
    }))
}

/// Células de `::before` ou `::after` de `c` em linha (vazio se não há `content` ou se é oculto).
fn pseudo_celulas(
    c: ElementRef<'_>,
    cs: &Style,
    qual: Pseudo,
    bg: Option<Rgb>,
    link: Option<u32>,
    cx: &Ctx<'_>,
) -> Vec<Cell> {
    let Some(ps) = compute_pseudo(c, cs, qual, cx.cascade) else {
        return Vec::new();
    };
    if ps.display == Display::None {
        return Vec::new();
    }
    let fbg = fundo_efetivo(&ps, bg);
    let mut v = Vec::new();
    push_text(
        &mut v,
        ps.content.as_deref().unwrap_or(""),
        face(&ps, fbg, link),
        ps.white,
        ps.transform,
    );
    v
}

/// Verdadeiro para os elementos substituídos (controles, mídia e gráficos), tratados como texto.
pub(crate) fn e_controle(nome: &str) -> bool {
    matches!(
        nome,
        "img"
            | "input"
            | "button"
            | "select"
            | "textarea"
            | "progress"
            | "meter"
            | "svg"
            | "canvas"
            | "video"
            | "audio"
    )
}

/// Células que representam um controle de formulário, gráfico ou mídia; `None` para os demais.
fn controle(
    c: ElementRef<'_>,
    cs: &Style,
    pai: &Style,
    bg: Option<Rgb>,
    link: Option<u32>,
    cx: &Ctx<'_>,
) -> Result<Option<Vec<Cell>>, String> {
    let v = c.value();
    let texto = match v.name() {
        "img" => match v.attr("alt").map(str::trim).filter(|a| !a.is_empty()) {
            Some(a) => format!("[imagem: {a}]"),
            None => "[imagem]".to_string(),
        },
        "input" | "button" | "select" | "textarea" => {
            return form::celulas(c, cs, bg, link, cx).map(Some);
        }
        "svg" => match media::texto_svg(c, cx)? {
            t if t.is_empty() => "[gráfico]".to_string(),
            t => format!("[gráfico: {t}]"),
        },
        "canvas" => marcador_midia("canvas", c, cx)?,
        "video" => marcador_midia("vídeo", c, cx)?,
        "audio" => marcador_midia("áudio", c, cx)?,
        "progress" | "meter" => {
            let barra = media::barra(c, cs, pai, bg, link, cx.largura.get());
            return Ok(Some(barra));
        }
        _ => return Ok(None),
    };
    let mut celulas = Vec::new();
    push_text(&mut celulas, &texto, face(cs, bg, link), WhiteSpace::Normal, Transform::None);
    Ok(Some(celulas))
}

/// `[canvas: texto]`, `[vídeo: texto]` ou `[áudio: texto]` com o conteúdo alternativo; sem texto,
/// só o nome entre colchetes.
fn marcador_midia(nome: &str, c: ElementRef<'_>, cx: &Ctx<'_>) -> Result<String, String> {
    let t = texto_visivel(c, cx)?;
    Ok(if t.is_empty() {
        format!("[{nome}]")
    } else {
        format!("[{nome}: {t}]")
    })
}

/// Texto visível de um elemento (sem `script`, `style`, `template`, `noscript` nem `hidden`),
/// com espaços colapsados e no máximo `max_cell_chars` caracteres.
pub(crate) fn texto_visivel(el: ElementRef<'_>, cx: &Ctx<'_>) -> Result<String, String> {
    let teto = cx.cascade.lim.max_cell_chars;
    let mut bruto = String::new();
    let mut pilha = filhos(el);
    pilha.reverse();
    while let Some(item) = pilha.pop() {
        cx.tick()?;
        match item {
            Item::Texto(t) => bruto.push_str(&t),
            Item::Elem(e) => {
                let v = e.value();
                if ua::sempre_oculto(v.name()) || v.attr("hidden").is_some() {
                    continue;
                }
                let mut sub = filhos(e);
                sub.reverse();
                pilha.extend(sub);
            }
        }
        if bruto.len() > teto.saturating_mul(4) {
            break;
        }
    }
    Ok(bruto
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(teto)
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::guard::Limits;
    use crate::testutil::{render_body, rodar_com, texto};
    use crate::Unlimited;

    fn todo(html: &str, largura: usize) -> String {
        render_body(html, "", largura).join("\n")
    }

    fn conta(html: &str, largura: usize, ch: char) -> usize {
        let saida = rodar_com(html, "", largura, &Limits::default(), &Unlimited).unwrap();
        saida
            .linhas
            .iter()
            .flat_map(|l| l.iter())
            .filter(|c| c.ch == ch)
            .count()
    }

    #[test]
    fn svg_mostra_os_textos_do_grafico() {
        let t = todo("<p><svg><rect/><text>Pico às 10h</text></svg></p>", 60);
        assert!(t.contains("[gráfico: Pico às 10h]"), "{t:?}");
    }

    #[test]
    fn svg_sem_texto_vira_marcador_simples() {
        let t = todo("<p><svg><rect/></svg></p>", 60);
        assert!(t.contains("[gráfico]"), "{t:?}");
    }

    #[test]
    fn svg_junta_titulo_descricao_e_textos_em_ordem() {
        let t = todo(
            "<svg><title>Vendas</title><desc>Mensal</desc><g><text>Jan</text></g></svg>",
            60,
        );
        assert!(t.contains("[gráfico: Vendas Mensal Jan]"), "{t:?}");
    }

    #[test]
    fn svg_limita_o_texto_a_80_caracteres() {
        let longo = "palavra ".repeat(40);
        let t = todo(&format!("<svg><text>{longo}</text></svg>"), 120);
        let inicio = t.find("[gráfico: ").unwrap_or(0) + "[gráfico: ".len();
        let resto = t.get(inicio..).unwrap_or("");
        let dentro = resto.split(']').next().unwrap_or("");
        assert!(dentro.starts_with("palavra"), "{t:?}");
        assert!(dentro.chars().count() <= 80, "{} chars: {t:?}", dentro.chars().count());
    }

    #[test]
    fn canvas_usa_o_conteudo_alternativo() {
        let com = todo("<canvas>Gráfico de linhas</canvas>", 60);
        assert!(com.contains("[canvas: Gráfico de linhas]"), "{com:?}");
        let vazio = todo("<canvas></canvas>", 60);
        assert!(vazio.contains("[canvas]"), "{vazio:?}");
    }

    #[test]
    fn video_e_audio_viram_marcadores() {
        let t = todo("<p><video></video> <audio>Trilha</audio></p>", 60);
        assert!(t.contains("[vídeo]"), "{t:?}");
        assert!(t.contains("[áudio: Trilha]"), "{t:?}");
    }

    #[test]
    fn iframe_continua_oculto() {
        let t = todo("<iframe>Secreto</iframe><p>ok</p>", 60);
        assert!(!t.contains("Secreto"), "{t:?}");
        assert!(t.contains("ok"), "{t:?}");
    }

    #[test]
    fn progress_vira_barra_de_20_colunas() {
        let saida = rodar_com(
            "<progress value=\"78\" max=\"100\">78%</progress>",
            "",
            40,
            &Limits::default(),
            &Unlimited,
        )
        .unwrap();
        assert_eq!(
            texto(&saida.linhas[0]),
            format!("{}{}", "█".repeat(16), "░".repeat(4))
        );
    }

    #[test]
    fn progress_usa_verde_por_padrao() {
        let saida = rodar_com(
            "<progress value=\"50\" max=\"100\"></progress>",
            "",
            40,
            &Limits::default(),
            &Unlimited,
        )
        .unwrap();
        let cor = saida.linhas[0]
            .iter()
            .find(|c| c.ch == '█')
            .map(|c| c.face.fg);
        assert_eq!(cor, Some(Some(Rgb(63, 185, 80))));
    }

    #[test]
    fn meter_proporcional_ao_intervalo() {
        // 287 de 0..500 = 57,4% de 20 colunas: 11 blocos cheios.
        let html = "<meter min=\"0\" max=\"500\" value=\"287\">287 de 500</meter>";
        assert_eq!(conta(html, 40, '█'), 11);
        assert!(!todo(html, 40).contains("287"));
    }

    #[test]
    fn progress_sem_valor_e_todo_vazio() {
        let html = "<progress max=\"100\"></progress>";
        assert_eq!(conta(html, 40, '░'), 20);
        assert_eq!(conta(html, 40, '█'), 0);
    }

    #[test]
    fn progress_sem_max_usa_um() {
        assert_eq!(conta("<progress value=\"0.5\"></progress>", 40, '█'), 10);
    }

    #[test]
    fn progress_com_width_em_px_limitado_ao_disponivel() {
        let html = "<progress value=\"100\" max=\"100\" style=\"width:480px\"></progress>";
        assert_eq!(conta(html, 40, '█'), 40);
    }

    #[test]
    fn progress_como_item_de_grid_vira_barra() {
        let html = "<div style=\"display:grid;grid-template-columns:1fr 60px\">\
            <span>Pedidos</span><progress value=\"50\" max=\"100\">50%</progress></div>";
        assert!(conta(html, 40, '█') > 0, "{}", todo(html, 40));
        assert!(!todo(html, 40).contains("50%"), "{}", todo(html, 40));
    }

    #[test]
    fn codigo_sem_padding_nao_ganha_espaco() {
        assert_eq!(todo("<p>use <code>x</code>.</p>", 40).trim_end(), "use x.");
    }

    #[test]
    fn kbd_sem_padding_fica_colado() {
        assert_eq!(todo("<p>tecla <kbd>Ctrl</kbd>+<kbd>K</kbd>.</p>", 40).trim_end(), "tecla Ctrl+K.");
    }

    #[test]
    fn pilula_com_padding_medio_ganha_um_espaco() {
        let html = "<p>a<a href=\"#x\" style=\"background:#0a0;padding:6px 16px\">Ir</a>b</p>";
        assert_eq!(todo(html, 40).trim_end(), "a Ir b");
    }

    #[test]
    fn pilula_com_padding_grande_ganha_dois_espacos() {
        let html = "<p>a<a href=\"#x\" style=\"background:#0a0;padding:6px 24px\">Ir</a>b</p>";
        assert_eq!(todo(html, 40).trim_end(), "a \u{a0}Ir\u{a0} b");
    }

    #[test]
    fn pilula_nao_separa_da_pontuacao_seguinte() {
        let html = "<p>chave <code style=\"padding:1px 5px\">sk_test</code>.</p>";
        assert_eq!(todo(html, 40).trim_end(), "chave sk_test.");
    }

    #[test]
    fn pilula_nao_separa_do_parentese_que_abre() {
        let html = "<p>veja (<code style=\"padding:1px 5px\">x</code>)</p>";
        assert_eq!(todo(html, 40).trim_end(), "veja (x)");
    }

    #[test]
    fn q_ganha_aspas_tipograficas() {
        assert_eq!(todo("<p>disse <q>oi</q>.</p>", 40).trim_end(), "disse “oi”.");
    }

    #[test]
    fn before_inline_com_content() {
        let t = render_body("<p class=\"x\">texto</p>", ".x::before{content:\"→ \"}", 40);
        assert_eq!(t[0], "→ texto");
    }

    #[test]
    fn after_de_link_com_content() {
        let t = render_body("<p><a href=\"#x\">Ir</a></p>", "a::after{content:\" ↗\"}", 40);
        assert_eq!(t[0], "Ir ↗");
    }

    #[test]
    fn content_com_escape_hex_decodifica() {
        let t = render_body("<p class=\"x\">texto</p>", ".x::before{content:\"\\201C\"}", 40);
        assert_eq!(t[0], "“texto");
    }

    #[test]
    fn content_com_counter_e_ignorado() {
        let t = render_body("<p class=\"x\">texto</p>", ".x::before{content:counter(n)}", 40);
        assert_eq!(t[0], "texto");
    }

    #[test]
    fn pseudo_display_none_some() {
        let t = render_body(
            "<p class=\"x\">texto</p>",
            ".x::before{content:\"a\";display:none}",
            40,
        );
        assert_eq!(t[0], "texto");
    }

    #[test]
    fn before_vazio_com_altura_e_fundo_vira_barra() {
        let s = rodar_com(
            "<div class=\"b\"></div>",
            ".b::before{content:\"\";display:block;height:16px;background:#222}",
            40,
            &Limits::default(),
            &Unlimited,
        )
        .unwrap();
        let primeira = s.linhas.first().and_then(|l| l.first()).map(|c| c.face.bg);
        assert_eq!(primeira, Some(Some(Rgb(34, 34, 34))), "{:?}", s.linhas.len());
    }
}
