//! Cores CSS: leitura de valores, mistura com alfa e contraste WCAG.

use crate::model::Rgb;

/// Cor lida de uma declaração CSS, com alfa entre 0.0 e 1.0.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Color {
    pub rgb: Rgb,
    pub alpha: f32,
}

/// Lê um valor de cor CSS. Devolve `None` para qualquer coisa que não seja cor.
/// Valores fora de faixa são limitados, nunca causam pânico.
pub fn parse(s: &str) -> Option<Color> {
    let t = s.trim().to_ascii_lowercase();
    if t.is_empty() {
        return None;
    }
    if let Some(h) = t.strip_prefix('#') {
        return hex(h);
    }
    if t == "transparent" {
        return Some(Color {
            rgb: Rgb(0, 0, 0),
            alpha: 0.0,
        });
    }
    if let Some(open) = t.find('(') {
        let func = t.get(..open)?.trim();
        let inner = t.get(open + 1..)?.strip_suffix(')')?;
        return match func {
            "rgb" | "rgba" => rgb_fn(inner),
            "hsl" | "hsla" => hsl_fn(inner),
            _ => None,
        };
    }
    named(&t).map(|rgb| Color { rgb, alpha: 1.0 })
}

/// `#rgb`, `#rgba`, `#rrggbb` e `#rrggbbaa` (já sem o `#`).
fn hex(h: &str) -> Option<Color> {
    if !h.is_ascii() {
        return None;
    }
    let d: Vec<u8> = h
        .bytes()
        .map(|b| (b as char).to_digit(16).map(|x| x as u8))
        .collect::<Option<Vec<u8>>>()?;
    let (rgb, alpha) = match d.as_slice() {
        [r, g, b] => (Rgb(r * 17, g * 17, b * 17), 255),
        [r, g, b, a] => (Rgb(r * 17, g * 17, b * 17), a * 17),
        [r1, r2, g1, g2, b1, b2] => (Rgb(r1 * 16 + r2, g1 * 16 + g2, b1 * 16 + b2), 255),
        [r1, r2, g1, g2, b1, b2, a1, a2] => (
            Rgb(r1 * 16 + r2, g1 * 16 + g2, b1 * 16 + b2),
            a1 * 16 + a2,
        ),
        _ => return None,
    };
    Some(Color {
        rgb,
        alpha: f32::from(alpha) / 255.0,
    })
}

/// Separa os argumentos de `rgb(...)`/`hsl(...)`: vírgula, espaço ou `/`.
fn args(inner: &str) -> Vec<&str> {
    inner
        .split(|c: char| c == ',' || c == '/' || c.is_whitespace())
        .filter(|s| !s.is_empty())
        .collect()
}

fn num(s: &str) -> Option<f32> {
    s.parse::<f32>().ok().filter(|x| x.is_finite())
}

/// Canal de cor: número (0 a 255) ou porcentagem.
fn channel(s: &str) -> Option<u8> {
    let v = match s.strip_suffix('%') {
        Some(p) => num(p)? * 2.55,
        None => num(s)?,
    };
    Some(v.clamp(0.0, 255.0).round() as u8)
}

/// Alfa: número (0 a 1) ou porcentagem.
fn alpha(s: &str) -> Option<f32> {
    let v = match s.strip_suffix('%') {
        Some(p) => num(p)? / 100.0,
        None => num(s)?,
    };
    Some(v.clamp(0.0, 1.0))
}

fn rgb_fn(inner: &str) -> Option<Color> {
    let a = args(inner);
    let (r, g, b, al) = match a.as_slice() {
        [r, g, b] => (r, g, b, None),
        [r, g, b, al] => (r, g, b, Some(al)),
        _ => return None,
    };
    let alpha = match al {
        Some(s) => alpha(s)?,
        None => 1.0,
    };
    Some(Color {
        rgb: Rgb(channel(r)?, channel(g)?, channel(b)?),
        alpha,
    })
}

/// `hsl(h, s%, l%)`: matiz em graus (sufixo `deg` opcional), saturação e luminosidade em porcentagem.
fn hsl_fn(inner: &str) -> Option<Color> {
    let a = args(inner);
    let (h, s, l, al) = match a.as_slice() {
        [h, s, l] => (h, s, l, None),
        [h, s, l, al] => (h, s, l, Some(al)),
        _ => return None,
    };
    let h = num(h.strip_suffix("deg").unwrap_or(h))?.rem_euclid(360.0);
    let s = (num(s.strip_suffix('%').unwrap_or(s))? / 100.0).clamp(0.0, 1.0);
    let l = (num(l.strip_suffix('%').unwrap_or(l))? / 100.0).clamp(0.0, 1.0);
    let alpha = match al {
        Some(x) => alpha(x)?,
        None => 1.0,
    };
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let hp = h / 60.0;
    let x = c * (1.0 - ((hp % 2.0) - 1.0).abs());
    let m = l - c / 2.0;
    let (r1, g1, b1) = match (hp as u32) % 6 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let to_u8 = |v: f32| ((v + m) * 255.0).clamp(0.0, 255.0).round() as u8;
    Some(Color {
        rgb: Rgb(to_u8(r1), to_u8(g1), to_u8(b1)),
        alpha,
    })
}

/// Nomes CSS: os 16 básicos do CSS 1 e os mais comuns (lista do spec).
fn named(name: &str) -> Option<Rgb> {
    let (r, g, b) = match name {
        "white" => (255, 255, 255),
        "black" => (0, 0, 0),
        "red" => (255, 0, 0),
        "green" => (0, 128, 0),
        "blue" => (0, 0, 255),
        "yellow" => (255, 255, 0),
        "orange" => (255, 165, 0),
        "purple" => (128, 0, 128),
        "gray" | "grey" => (128, 128, 128),
        "cyan" | "aqua" => (0, 255, 255),
        "magenta" | "fuchsia" => (255, 0, 255),
        "pink" => (255, 192, 203),
        "silver" => (192, 192, 192),
        "navy" => (0, 0, 128),
        "teal" => (0, 128, 128),
        "lime" => (0, 255, 0),
        "maroon" => (128, 0, 0),
        "olive" => (128, 128, 0),
        "gold" => (255, 215, 0),
        "tomato" => (255, 99, 71),
        "coral" => (255, 127, 80),
        "salmon" => (250, 128, 114),
        "crimson" => (220, 20, 60),
        "indigo" => (75, 0, 130),
        "violet" => (238, 130, 238),
        "turquoise" => (64, 224, 208),
        "tan" => (210, 180, 140),
        "brown" => (165, 42, 42),
        "darkgray" | "darkgrey" => (169, 169, 169),
        "lightgray" | "lightgrey" => (211, 211, 211),
        "dimgray" | "dimgrey" => (105, 105, 105),
        "slategray" | "slategrey" => (112, 128, 144),
        "steelblue" => (70, 130, 180),
        "royalblue" => (65, 105, 225),
        "skyblue" => (135, 206, 235),
        "forestgreen" => (34, 139, 34),
        "seagreen" => (46, 139, 87),
        "darkgreen" => (0, 100, 0),
        "darkred" => (139, 0, 0),
        "darkblue" => (0, 0, 139),
        "orangered" => (255, 69, 0),
        "khaki" => (240, 230, 140),
        "beige" => (245, 245, 220),
        "ivory" => (255, 255, 240),
        "lavender" => (230, 230, 250),
        "plum" => (221, 160, 221),
        "orchid" => (218, 112, 214),
        "wheat" => (245, 222, 179),
        _ => return None,
    };
    Some(Rgb(r, g, b))
}

/// Mistura `top` (com alfa) sobre `under`; sem fundo, assume branco.
pub fn blend(top: Color, under: Option<Rgb>) -> Rgb {
    let Rgb(ur, ug, ub) = under.unwrap_or(Rgb(255, 255, 255));
    let Rgb(tr, tg, tb) = top.rgb;
    let a = top.alpha.clamp(0.0, 1.0);
    let mix = |t: u8, u: u8| (f32::from(t) * a + f32::from(u) * (1.0 - a)).round() as u8;
    Rgb(mix(tr, ur), mix(tg, ug), mix(tb, ub))
}

/// Luminância relativa WCAG, de 0.0 a 1.0.
pub fn luminance(c: Rgb) -> f32 {
    let lin = |v: u8| {
        let x = f32::from(v) / 255.0;
        if x <= 0.03928 {
            x / 12.92
        } else {
            ((x + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * lin(c.0) + 0.7152 * lin(c.1) + 0.0722 * lin(c.2)
}

/// Razão de contraste WCAG (1.0 a 21.0).
pub fn contrast(a: Rgb, b: Rgb) -> f32 {
    let la = luminance(a);
    let lb = luminance(b);
    let (hi, lo) = if la > lb { (la, lb) } else { (lb, la) };
    (hi + 0.05) / (lo + 0.05)
}

/// Corrige a cor do texto para não ficar ilegível sobre o fundo.
/// Sem fundo, mantém `fg`. Sem `fg` sobre fundo, usa preto ou branco.
/// Com razão abaixo de 2.0, troca pelo preto ou branco de maior contraste.
pub fn fix_contrast(fg: Option<Rgb>, bg: Option<Rgb>) -> Option<Rgb> {
    let Some(bg) = bg else {
        return fg;
    };
    let black = Rgb(0, 0, 0);
    let white = Rgb(255, 255, 255);
    match fg {
        None => Some(if luminance(bg) > 0.5 { black } else { white }),
        Some(f) if contrast(f, bg) < 2.0 => {
            if contrast(black, bg) >= contrast(white, bg) {
                Some(black)
            } else {
                Some(white)
            }
        }
        Some(f) => Some(f),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cor(s: &str) -> Color {
        parse(s).unwrap()
    }

    #[test]
    fn hex_curto_e_longo() {
        assert_eq!(parse("#f00").map(|c| c.rgb), Some(Rgb(255, 0, 0)));
        assert_eq!(parse("#0d1117").map(|c| c.rgb), Some(Rgb(13, 17, 23)));
        assert!((cor("#ff000080").alpha - 0.5).abs() < 0.01);
    }

    #[test]
    fn rgb_e_rgba() {
        assert_eq!(cor("rgb(217, 119, 87)").rgb, Rgb(217, 119, 87));
        assert!((cor("rgba(63,185,80,.12)").alpha - 0.12).abs() < 0.01);
        assert!((cor("rgb(10 20 30 / 50%)").alpha - 0.5).abs() < 0.01);
    }

    #[test]
    fn hsl_vermelho() {
        assert_eq!(cor("hsl(0, 100%, 50%)").rgb, Rgb(255, 0, 0));
    }

    #[test]
    fn nomes() {
        assert!(parse("Coral").is_some());
        assert!(parse("tomato").is_some());
        assert!((cor("transparent").alpha - 0.0).abs() < f32::EPSILON);
    }

    #[test]
    fn invalido_e_none() {
        for s in ["banana", "#12", "rgb(1,2)", "", "var(--x)"] {
            assert_eq!(parse(s), None, "{s:?}");
        }
    }

    #[test]
    fn valores_fora_de_faixa_sao_limitados() {
        assert_eq!(cor("rgb(999,-5,300)").rgb, Rgb(255, 0, 255));
    }

    #[test]
    fn mistura_com_alfa() {
        let preto = Color { rgb: Rgb(0, 0, 0), alpha: 0.5 };
        let c = blend(preto, Some(Rgb(255, 255, 255)));
        assert!((127..=128).contains(&c.0), "{c:?}");
        assert_eq!(c.0, c.1);
        assert_eq!(c.1, c.2);
    }

    #[test]
    fn contraste_texto_igual_ao_fundo() {
        let c = Rgb(120, 120, 120);
        let fixed = fix_contrast(Some(c), Some(c)).unwrap();
        assert_ne!(fixed, c);
        assert!(contrast(fixed, c) >= 4.5);
    }

    #[test]
    fn fundo_escuro_sem_cor_vira_branco() {
        assert_eq!(
            fix_contrast(None, Some(Rgb(13, 17, 23))),
            Some(Rgb(255, 255, 255))
        );
    }

    #[test]
    fn sem_fundo_nao_mexe() {
        assert_eq!(fix_contrast(Some(Rgb(1, 2, 3)), None), Some(Rgb(1, 2, 3)));
    }
}
