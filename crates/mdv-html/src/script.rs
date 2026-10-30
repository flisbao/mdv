//! `<sup>` e `<sub>`: dígitos e sinais viram sobrescrito/subscrito Unicode; quando nenhum
//! caractere do trecho tem equivalente, o texto ganha o prefixo `^` (sobrescrito) ou `_`
//! (subscrito). A decisão vale para o trecho inteiro do elemento, não para cada nó.

use crate::ctx::Ctx;
use crate::inline::{self, Item};
use crate::ua;
use scraper::ElementRef;

/// Pares (caractere normal, equivalente de sobrescrito).
const SOBRESCRITO: [(char, char); 15] = [
    ('0', '⁰'),
    ('1', '¹'),
    ('2', '²'),
    ('3', '³'),
    ('4', '⁴'),
    ('5', '⁵'),
    ('6', '⁶'),
    ('7', '⁷'),
    ('8', '⁸'),
    ('9', '⁹'),
    ('+', '⁺'),
    ('-', '⁻'),
    ('(', '⁽'),
    (')', '⁾'),
    ('=', '⁼'),
];

/// Pares (caractere normal, equivalente de subscrito).
const SUBSCRITO: [(char, char); 15] = [
    ('0', '₀'),
    ('1', '₁'),
    ('2', '₂'),
    ('3', '₃'),
    ('4', '₄'),
    ('5', '₅'),
    ('6', '₆'),
    ('7', '₇'),
    ('8', '₈'),
    ('9', '₉'),
    ('+', '₊'),
    ('-', '₋'),
    ('(', '₍'),
    (')', '₎'),
    ('=', '₌'),
];

/// Como um trecho de `<sup>`/`<sub>` é escrito. `mapear` diz se os caracteres viram equivalentes
/// Unicode (senão ganha o prefixo `^`/`_`). `sup` distingue sobrescrito de subscrito.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Script {
    pub sup: bool,
    pub mapear: bool,
}

/// Equivalente de sobrescrito ou subscrito de `c`, se existir.
pub fn equivalente(c: char, sup: bool) -> Option<char> {
    let tabela = if sup { &SOBRESCRITO } else { &SUBSCRITO };
    tabela.iter().find(|(a, _)| *a == c).map(|(_, b)| *b)
}

/// Verdadeiro se algum caractere de `texto` tem equivalente de sobrescrito/subscrito.
pub fn mapear(texto: &str, sup: bool) -> bool {
    texto.chars().any(|c| equivalente(c, sup).is_some())
}

/// Troca cada caractere pelo equivalente; os sem equivalente ficam como estão.
pub fn converte(texto: &str, sup: bool) -> String {
    texto
        .chars()
        .map(|c| equivalente(c, sup).unwrap_or(c))
        .collect()
}

/// Prefixo de um trecho que não pode ser mapeado: `^` ou `_`.
pub fn prefixo(sup: bool) -> char {
    if sup {
        '^'
    } else {
        '_'
    }
}

/// Texto de todo o subtree de `el` (sem colapsar espaços), sem elementos ocultos, com teto de
/// bytes para não crescer sem limite.
pub fn texto_bruto(el: ElementRef<'_>, cx: &Ctx<'_>) -> Result<String, String> {
    let teto = cx.cascade.lim.max_cell_chars.saturating_mul(4);
    let mut out = String::new();
    let mut pilha = inline::filhos(el);
    pilha.reverse();
    while let Some(item) = pilha.pop() {
        cx.tick()?;
        match item {
            Item::Texto(t) => out.push_str(&t),
            Item::Elem(e) => {
                let v = e.value();
                if ua::sempre_oculto(v.name()) || v.attr("hidden").is_some() {
                    continue;
                }
                let mut sub = inline::filhos(e);
                sub.reverse();
                pilha.extend(sub);
            }
        }
        if out.len() > teto {
            break;
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use crate::guard::Limits;
    use crate::testutil::{render_body, rodar_com};
    use crate::Unlimited;

    fn primeira(html: &str) -> String {
        render_body(html, "", 60).first().cloned().unwrap_or_default()
    }

    #[test]
    fn sup_digito_vira_sobrescrito() {
        assert_eq!(primeira("<p>x<sup>2</sup></p>"), "x²");
    }

    #[test]
    fn sub_digitos_viram_subscrito() {
        assert_eq!(primeira("<p><sub>10</sub></p>"), "₁₀");
    }

    #[test]
    fn sup_sem_equivalente_ganha_circunflexo() {
        assert_eq!(primeira("<p><sup>nota</sup></p>"), "^nota");
    }

    #[test]
    fn sub_sem_equivalente_ganha_underline() {
        assert_eq!(primeira("<p><sub>nota</sub></p>"), "_nota");
    }

    #[test]
    fn sinais_tem_equivalente() {
        assert_eq!(primeira("<p><sup>+-()=</sup></p>"), "⁺⁻⁽⁾⁼");
        assert_eq!(primeira("<p><sub>+-()=</sub></p>"), "₊₋₍₎₌");
    }

    #[test]
    fn misto_mapeia_so_o_que_tem_equivalente() {
        assert_eq!(primeira("<p><sup>2a</sup></p>"), "²a");
    }

    #[test]
    fn sup_com_link_mantem_o_link() {
        let s = rodar_com(
            "<p><sup><a href=\"#n1\">1</a></sup></p>",
            "",
            60,
            &Limits::default(),
            &Unlimited,
        )
        .unwrap();
        assert_eq!(s.linhas[0].iter().map(|c| c.ch).collect::<String>().trim_end(), "¹");
        assert_eq!(s.links, vec!["#n1".to_string()]);
    }
}
