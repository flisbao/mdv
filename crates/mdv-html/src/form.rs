//! Controles de formulário em texto: `input` (cada tipo), `select`, `textarea` e `button`.
//! Cada controle vira `[ valor ]`, `[x]`, `( )`, `[──●────]` etc., com o estilo do elemento.
//! O `textarea` é um bloco de `rows` linhas (padrão 3, máximo 6); os demais são texto corrido.

use crate::ctx::Ctx;
use crate::inline::{self, celula, face, texto_visivel, Item};
use crate::length::{px_to_cols, Len};
use crate::model::{Cell, Face};
use crate::style::{Style, Transform, WhiteSpace};
use crate::text::push_text;
use scraper::node::Element;
use scraper::ElementRef;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// Largura, em colunas, de um campo sem valor, sem `size` e sem `width` em px.
const CAMPO_PADRAO: usize = 12;
/// Teto de colunas de um campo de texto e de bolinhas de senha.
const CAMPO_MAX: usize = 40;
/// Colunas da trilha de `input type=range`.
const TRILHA: usize = 7;
/// Linhas de um `textarea` sem `rows`, e teto de `rows`.
const LINHAS_PADRAO: usize = 3;
const LINHAS_MAX: usize = 6;
/// Largura mínima, em colunas, da caixa de um `textarea`.
const CAIXA_MIN: usize = 20;

/// Células de um controle de formulário (`input`, `select`, `textarea`, `button`).
pub fn celulas(
    c: ElementRef<'_>,
    cs: &Style,
    bg: Option<crate::model::Rgb>,
    link: Option<u32>,
    cx: &Ctx<'_>,
) -> Result<Vec<Cell>, String> {
    let f = face(cs, bg, link);
    match c.value().name() {
        "input" => Ok(input(c, cs, f, cx)),
        "select" => {
            let escolhida = opcao_escolhida(c, cx)?;
            Ok(texto_com(&rotulo(&format!("{escolhida} ▾")), f))
        }
        "textarea" => textarea(c, f, cx),
        _ => Ok(texto_com(&rotulo(&texto_visivel(c, cx)?), f)),
    }
}

/// Células de um `input`, conforme o `type` (texto por padrão).
fn input(c: ElementRef<'_>, cs: &Style, f: Face, cx: &Ctx<'_>) -> Vec<Cell> {
    let v = c.value();
    let teto = cx.cascade.lim.max_cell_chars;
    let tipo = v.attr("type").unwrap_or("text").trim().to_ascii_lowercase();
    let t = match tipo.as_str() {
        "hidden" => return Vec::new(),
        "checkbox" => marca(v.attr("checked").is_some(), "[x]", "[ ]"),
        "radio" => marca(v.attr("checked").is_some(), "(•)", "( )"),
        "submit" | "reset" | "button" | "image" => {
            let padrao = match tipo.as_str() {
                "submit" => "Enviar",
                "reset" => "Limpar",
                "image" => "Imagem",
                _ => "Botão",
            };
            rotulo(&atributo(v, "value", teto).unwrap_or_else(|| padrao.to_string()))
        }
        "file" => rotulo("Escolher arquivo"),
        "color" => rotulo(&atributo(v, "value", teto).unwrap_or_else(|| "#000000".to_string())),
        "range" => faixa(v),
        _ => return campo(c, cs, f, tipo == "password", teto),
    };
    texto_com(&t, f)
}

/// Campo de texto (ou senha): o valor; sem valor, o placeholder esmaecido; sem nenhum, uma linha
/// de sublinhados com a largura do `size` ou da `width` em px.
fn campo(c: ElementRef<'_>, cs: &Style, f: Face, senha: bool, teto: usize) -> Vec<Cell> {
    let v = c.value();
    let valor_bruto = atributo(v, "value", teto).unwrap_or_default();
    let valor = valor_bruto.trim();
    if !valor.is_empty() {
        let mostrado = if senha {
            "•".repeat(valor.chars().count().min(CAMPO_MAX))
        } else {
            valor.to_string()
        };
        return texto_com(&rotulo(&mostrado), f);
    }
    let dica = atributo(v, "placeholder", teto).unwrap_or_default();
    if !dica.trim().is_empty() {
        return texto_com(&rotulo(&dica), esmaecido(f));
    }
    let traços = "_".repeat(largura_campo(v, cs));
    texto_com(&format!("[ {traços} ]"), f)
}

/// Colunas de um campo sem valor: `size`, senão a `width` em px dividida por 8, senão 12; no
/// máximo `CAMPO_MAX`.
fn largura_campo(v: &Element, cs: &Style) -> usize {
    let n = v
        .attr("size")
        .and_then(|s| s.trim().parse::<usize>().ok())
        .or_else(|| match cs.width {
            Some(Len::Px(px)) => Some(px_to_cols(px)),
            _ => None,
        })
        .unwrap_or(CAMPO_PADRAO);
    n.clamp(1, CAMPO_MAX)
}

/// Valor de atributo, cortado em `teto` caracteres (atributo enorme não vira célula sem limite).
fn atributo(v: &Element, nome: &str, teto: usize) -> Option<String> {
    v.attr(nome).map(|s| s.chars().take(teto).collect())
}

/// Trilha de `input type=range`: `[──●────]`, com o cursor na fração entre `min` e `max`.
fn faixa(v: &Element) -> String {
    let minimo = num(v.attr("min")).unwrap_or(0.0);
    let maximo = num(v.attr("max"))
        .filter(|m| *m > minimo)
        .unwrap_or(minimo + 100.0);
    let valor = num(v.attr("value"))
        .unwrap_or((minimo + maximo) / 2.0)
        .max(minimo)
        .min(maximo);
    let fracao = (valor - minimo) / (maximo - minimo);
    let ultimo = (TRILHA - 1) as f32;
    let pos = (fracao * ultimo).round().max(0.0).min(ultimo) as usize;
    let trilha: String = (0..TRILHA)
        .map(|i| if i == pos { '●' } else { '─' })
        .collect();
    format!("[{trilha}]")
}

/// Opção escolhida de um `select`: a marcada com `selected`, senão a primeira.
fn opcao_escolhida(el: ElementRef<'_>, cx: &Ctx<'_>) -> Result<String, String> {
    let mut opcoes: Vec<ElementRef<'_>> = Vec::new();
    for item in inline::filhos(el) {
        cx.tick()?;
        let Item::Elem(c) = item else { continue };
        match c.value().name() {
            "option" => opcoes.push(c),
            "optgroup" => {
                for sub in inline::filhos(c) {
                    cx.tick()?;
                    if let Item::Elem(o) = sub {
                        if o.value().name() == "option" {
                            opcoes.push(o);
                        }
                    }
                }
            }
            _ => {}
        }
    }
    let escolhida = opcoes
        .iter()
        .find(|o| o.value().attr("selected").is_some())
        .or(opcoes.first());
    match escolhida {
        Some(o) => texto_visivel(*o, cx),
        None => Ok(String::new()),
    }
}

/// Caixa de `textarea`: `rows` linhas (padrão 3, máximo 6), com o texto ou o placeholder na
/// primeira. A caixa tem pelo menos `CAIXA_MIN` colunas e o preenchimento usa espaço sem quebra.
fn textarea(c: ElementRef<'_>, f: Face, cx: &Ctx<'_>) -> Result<Vec<Cell>, String> {
    let v = c.value();
    let linhas = v
        .attr("rows")
        .and_then(|r| r.trim().parse::<usize>().ok())
        .unwrap_or(LINHAS_PADRAO)
        .clamp(1, LINHAS_MAX);
    let conteudo = texto_visivel(c, cx)?;
    let dica = atributo(v, "placeholder", cx.cascade.lim.max_cell_chars).unwrap_or_default();
    let dica = dica.trim();
    let (meio, fmeio) = if !conteudo.is_empty() {
        (conteudo, f)
    } else if !dica.is_empty() {
        (dica.to_string(), esmaecido(f))
    } else {
        (String::new(), f)
    };
    // A caixa tem pelo menos `CAIXA_MIN` colunas, mas nunca passa da largura disponível.
    let disponivel = match cx.largura.get() {
        0 => usize::MAX,
        d => d,
    };
    let w = CAIXA_MIN
        .max(4 + UnicodeWidthStr::width(meio.as_str()))
        .min(disponivel.max(4));
    let meio = corta_largura(&meio, w.saturating_sub(4));
    let mut out = caixa_primeira(&meio, fmeio, f, w);
    for _ in 1..linhas {
        out.push(celula('\n', Face::default()));
        out.extend(texto_com(&format!("[{}]", "\u{a0}".repeat(w.saturating_sub(2))), f));
    }
    Ok(out)
}

/// Prefixo de `s` com no máximo `max` colunas.
fn corta_largura(s: &str, max: usize) -> String {
    let mut out = String::new();
    let mut usado = 0usize;
    for ch in s.chars() {
        let cw = UnicodeWidthChar::width(ch).unwrap_or(0);
        if usado.saturating_add(cw) > max {
            break;
        }
        usado += cw;
        out.push(ch);
    }
    out
}

/// Primeira linha da caixa do `textarea`: `[ meio   ]` com `w` colunas.
fn caixa_primeira(meio: &str, fmeio: Face, f: Face, w: usize) -> Vec<Cell> {
    let usado = 4 + UnicodeWidthStr::width(meio);
    let mut out = texto_com("[ ", f);
    out.extend(texto_com(meio, fmeio));
    out.extend(texto_com(
        &format!("{} ]", "\u{a0}".repeat(w.saturating_sub(usado))),
        f,
    ));
    out
}

/// `[ texto ]`, ou `[ ]` quando o texto está vazio.
fn rotulo(t: &str) -> String {
    let t = t.trim();
    if t.is_empty() {
        "[ ]".to_string()
    } else {
        format!("[ {t} ]")
    }
}

/// `a` quando `marcado`, senão `b`.
fn marca(marcado: bool, a: &str, b: &str) -> String {
    if marcado { a } else { b }.to_string()
}

/// Estilo esmaecido (texto de dica).
fn esmaecido(f: Face) -> Face {
    Face { dim: true, ..f }
}

/// Texto simples como células com o estilo `f`; espaços e quebras seguem as regras normais.
fn texto_com(t: &str, f: Face) -> Vec<Cell> {
    let mut v = Vec::new();
    push_text(&mut v, t, f, WhiteSpace::Normal, Transform::None);
    v
}

fn num(s: Option<&str>) -> Option<f32> {
    s?.trim().parse::<f32>().ok().filter(|x| x.is_finite())
}

#[cfg(test)]
mod tests {
    use crate::guard::Limits;
    use crate::testutil::{faces, render_body, rodar_com};
    use crate::Unlimited;

    fn linhas(html: &str, largura: usize) -> Vec<String> {
        render_body(html, "", largura)
    }

    fn primeira(html: &str) -> String {
        linhas(html, 60).first().cloned().unwrap_or_default()
    }

    #[test]
    fn input_de_texto_mostra_o_valor() {
        let l = primeira("<p><input type=\"email\" value=\"voce@exemplo.com\"></p>");
        assert!(l.contains("[ voce@exemplo.com ]"), "{l:?}");
    }

    #[test]
    fn input_sem_valor_mostra_placeholder_esmaecido() {
        let html = "<p><input type=\"tel\" placeholder=\"(41) 90000-0000\"></p>";
        let l = primeira(html);
        assert!(l.contains("[ (41) 90000-0000 ]"), "{l:?}");
        let s = rodar_com(html, "", 60, &Limits::default(), &Unlimited).unwrap();
        let f = faces(&s.linhas, '(');
        assert!(f.first().is_some_and(|f| f.dim), "placeholder deve ser esmaecido");
    }

    #[test]
    fn input_sem_valor_nem_placeholder_vira_linha_de_12() {
        let l = primeira("<p><input name=\"x\"></p>");
        assert!(l.contains(&format!("[ {} ]", "_".repeat(12))), "{l:?}");
    }

    #[test]
    fn input_size_define_a_linha() {
        let l = primeira("<p><input size=\"20\"></p>");
        assert!(l.contains(&format!("[ {} ]", "_".repeat(20))), "{l:?}");
    }

    #[test]
    fn input_largura_css_em_px_dividida_por_8() {
        let l = primeira("<p><input style=\"width:240px\"></p>");
        assert!(l.contains(&format!("[ {} ]", "_".repeat(30))), "{l:?}");
    }

    #[test]
    fn input_linha_limitada_a_40() {
        let l = primeira("<p><input size=\"200\"></p>");
        assert!(l.contains(&format!("[ {} ]", "_".repeat(40))), "{l:?}");
    }

    #[test]
    fn tipos_de_texto_compartilham_o_campo() {
        for tipo in ["url", "search", "date", "number", "time", "text"] {
            let html = format!("<p><input type=\"{tipo}\" value=\"abc\"></p>");
            assert!(primeira(&html).contains("[ abc ]"), "tipo {tipo}");
        }
    }

    #[test]
    fn senha_mostra_uma_bolinha_por_caractere() {
        let l = primeira("<p><input type=\"password\" value=\"segred\"></p>");
        assert!(l.contains("[ •••••• ]"), "{l:?}");
        assert!(!l.contains("segred"), "{l:?}");
    }

    #[test]
    fn checkbox_marcado_e_desmarcado() {
        let l = primeira("<p><input type=\"checkbox\"> <input type=\"checkbox\" checked></p>");
        assert!(l.contains("[ ] [x]"), "{l:?}");
    }

    #[test]
    fn radio_usa_parenteses() {
        let l = primeira("<p><input type=\"radio\"> <input type=\"radio\" checked></p>");
        assert!(l.contains("( ) (•)"), "{l:?}");
    }

    #[test]
    fn botoes_submit_reset_e_button() {
        let l = primeira(
            "<p><input type=\"submit\"> <input type=\"reset\"> <input type=\"button\" value=\"Ok\"></p>",
        );
        assert!(l.contains("[ Enviar ] [ Limpar ] [ Ok ]"), "{l:?}");
        let s = primeira("<p><input type=\"submit\" value=\"Salvar\"></p>");
        assert!(s.contains("[ Salvar ]"), "{s:?}");
    }

    #[test]
    fn hidden_nao_aparece() {
        let l = primeira("<p>a<input type=\"hidden\" value=\"segredo\">b</p>");
        assert!(l.contains("ab"), "{l:?}");
        assert!(!l.contains("segredo"), "{l:?}");
    }

    #[test]
    fn range_mostra_a_trilha_com_o_cursor() {
        let l = primeira("<p><input type=\"range\" min=\"0\" max=\"10000\" value=\"2500\"></p>");
        assert!(l.contains("[──●────]"), "{l:?}");
    }

    #[test]
    fn color_mostra_o_hex() {
        let l = primeira("<p><input type=\"color\" value=\"#2563eb\"></p>");
        assert!(l.contains("[ #2563eb ]"), "{l:?}");
    }

    #[test]
    fn file_vira_botao_de_escolha() {
        let l = primeira("<p><input type=\"file\"></p>");
        assert!(l.contains("[ Escolher arquivo ]"), "{l:?}");
    }

    #[test]
    fn select_mostra_a_opcao_selecionada() {
        let com = primeira(
            "<p><select><option>Um</option><option selected>Dois</option></select></p>",
        );
        assert!(com.contains("[ Dois ▾ ]"), "{com:?}");
        let sem = primeira("<p><select><option>Um</option><option>Dois</option></select></p>");
        assert!(sem.contains("[ Um ▾ ]"), "{sem:?}");
    }

    #[test]
    fn textarea_tem_rows_linhas() {
        let l = linhas("<textarea rows=\"4\"></textarea>", 40);
        assert_eq!(l.iter().filter(|s| s.starts_with('[')).count(), 4, "{l:?}");
    }

    #[test]
    fn textarea_padrao_3_e_teto_6() {
        let padrao = linhas("<textarea></textarea>", 40);
        assert_eq!(padrao.iter().filter(|s| s.starts_with('[')).count(), 3, "{padrao:?}");
        let alto = linhas("<textarea rows=\"10\"></textarea>", 40);
        assert_eq!(alto.iter().filter(|s| s.starts_with('[')).count(), 6, "{alto:?}");
    }

    #[test]
    fn textarea_tem_no_minimo_20_colunas() {
        let l = linhas("<textarea>oi</textarea>", 40);
        assert_eq!(l[0].chars().count(), 20, "{l:?}");
        assert!(l[0].starts_with("[ oi"), "{l:?}");
    }

    #[test]
    fn textarea_cabe_na_largura_disponivel() {
        let l = linhas(
            "<textarea placeholder=\"Mensagem muito longa para caber aqui\"></textarea>",
            24,
        );
        assert_eq!(l.iter().filter(|s| s.starts_with('[')).count(), 3, "{l:?}");
        assert!(l.iter().all(|s| s.chars().count() <= 24), "{l:?}");
    }

    #[test]
    fn label_e_texto_corrido() {
        let l = primeira("<p><label>Nome <input type=\"text\"></label> ok</p>");
        assert_eq!(l, format!("Nome [ {} ] ok", "_".repeat(12)));
    }

    #[test]
    fn legend_vira_primeira_linha_em_negrito() {
        let html = "<fieldset><legend>Dados</legend><input type=\"text\"></fieldset>";
        let l = linhas(html, 40);
        assert_eq!(l[0], "Dados", "{l:?}");
        let s = rodar_com(html, "", 40, &Limits::default(), &Unlimited).unwrap();
        assert!(s.linhas[0][0].face.bold);
    }
}
