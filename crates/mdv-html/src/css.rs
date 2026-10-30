//! Folha de estilo mínima: regras `seletor { declarações }`, `@media` de um nível e declarações de `style=""`.

use crate::guard::Limits;
use crate::selector::{self, Pseudo, Selector};
use std::rc::Rc;

/// Declaração já aparada: nome em minúsculas, valor sem `!important`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Declaration {
    pub name: String,
    pub value: String,
}

/// Regra: um seletor (vírgulas viram regras separadas), o pseudoelemento se houver e as declarações.
#[derive(Debug)]
pub struct Rule {
    pub selector: Selector,
    pub pseudo: Option<Pseudo>,
    pub decls: Rc<Vec<Declaration>>,
    pub order: usize,
}

/// Todas as regras da folha, na ordem do arquivo.
#[derive(Debug, Default)]
pub struct Sheet {
    pub rules: Vec<Rule>,
}

/// Lê uma folha de estilo. `viewport_px` decide as condições de `@media`.
pub fn parse_sheet(css: &str, viewport_px: u32, lim: &Limits) -> Sheet {
    let limpo = sem_comentarios(corta(css, lim.max_css_bytes));
    let mut leitura = Leitura {
        lim,
        viewport: viewport_px,
        rules: Vec::new(),
    };
    leitura.blocos(&limpo, 0);
    Sheet {
        rules: leitura.rules,
    }
}

/// Lê declarações de `style=""` com os tetos padrão.
pub fn parse_declarations(s: &str) -> Vec<Declaration> {
    declaracoes(s, &Limits::default())
}

struct Leitura<'a> {
    lim: &'a Limits,
    viewport: u32,
    rules: Vec<Rule>,
}

impl Leitura<'_> {
    /// Percorre blocos de nível de topo (`nivel` 0) ou dentro de um `@media` (`nivel` 1).
    fn blocos(&mut self, css: &str, nivel: u8) {
        let mut i = 0usize;
        loop {
            if self.rules.len() >= self.lim.max_rules {
                return;
            }
            let Some(resto) = css.get(i..) else {
                return;
            };
            let aparado = resto.trim_start();
            i = css.len().saturating_sub(aparado.len());
            let Some(c) = aparado.chars().next() else {
                return;
            };
            if c == '}' {
                i = i.saturating_add(1);
                continue;
            }
            if c == '@' {
                let Some(p) = aparado.find([';', '{']) else {
                    return;
                };
                if aparado.get(p..p.saturating_add(1)) == Some(";") {
                    i = i.saturating_add(p).saturating_add(1);
                    continue;
                }
                let cabeca = aparado.get(1..p).unwrap_or("").trim();
                let fim_nome = cabeca
                    .find(|c: char| !c.is_ascii_alphabetic())
                    .unwrap_or(cabeca.len());
                let nome = cabeca.get(..fim_nome).unwrap_or("");
                let cond = cabeca.get(fim_nome..).unwrap_or("");
                let (corpo, fim) = bloco(css, i.saturating_add(p));
                if nome.eq_ignore_ascii_case("media") && nivel == 0 && media_ok(cond, self.viewport) {
                    self.blocos(corpo, 1);
                }
                i = fim;
                continue;
            }
            let Some(p) = aparado.find('{') else {
                return;
            };
            let antes = aparado.get(..p).unwrap_or("");
            // Um `}` solto antes do seletor não faz parte dele.
            let prelude = match antes.rfind('}') {
                Some(k) => antes.get(k.saturating_add(1)..).unwrap_or(""),
                None => antes,
            };
            let (corpo, fim) = bloco(css, i.saturating_add(p));
            self.regra(prelude.trim(), corpo);
            i = fim;
        }
    }

    /// Cria uma regra por seletor separado por vírgula.
    fn regra(&mut self, prelude: &str, corpo: &str) {
        let decls = Rc::new(declaracoes(corpo, self.lim));
        for pedaco in prelude.split(',') {
            if self.rules.len() >= self.lim.max_rules {
                return;
            }
            let texto = pedaco.trim();
            let (sel, pseudo) = match selector::parse_pseudo(texto) {
                Some((s, p)) => (s, Some(p)),
                None => match selector::parse(texto) {
                    Some(s) => (s, None),
                    None => continue,
                },
            };
            let order = self.rules.len();
            self.rules.push(Rule {
                selector: sel,
                pseudo,
                decls: Rc::clone(&decls),
                order,
            });
        }
    }
}

/// Corpo do bloco que abre em `abre` (índice do `{`) e índice logo depois do `}` que o fecha.
/// Bloco sem fechamento vai até o fim do texto.
fn bloco(css: &str, abre: usize) -> (&str, usize) {
    let mut d = 0usize;
    for (k, c) in css.bytes().enumerate().skip(abre) {
        match c {
            b'{' => d = d.saturating_add(1),
            b'}' => {
                d = d.saturating_sub(1);
                if d == 0 {
                    return (css.get(abre.saturating_add(1)..k).unwrap_or(""), k.saturating_add(1));
                }
            }
            _ => {}
        }
    }
    (css.get(abre.saturating_add(1)..).unwrap_or(""), css.len())
}

/// Prefixo de `s` com no máximo `n` bytes, sem cortar um caractere ao meio.
fn corta(s: &str, n: usize) -> &str {
    let mut i = n.min(s.len());
    while !s.is_char_boundary(i) {
        i = i.saturating_sub(1);
    }
    s.get(..i).unwrap_or("")
}

/// Tira os comentários `/* */`; um comentário sem fim descarta o resto.
fn sem_comentarios(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut resto = s;
    while let Some(i) = resto.find("/*") {
        out.push_str(resto.get(..i).unwrap_or(""));
        let depois = resto.get(i.saturating_add(2)..).unwrap_or("");
        match depois.find("*/") {
            Some(j) => resto = depois.get(j.saturating_add(2)..).unwrap_or(""),
            None => return out,
        }
    }
    out.push_str(resto);
    out
}

/// Condição de `@media` suportada: vazia, `all`, `screen`, ou `(min-width|max-width: Npx)` combinadas por `and`.
fn media_ok(cond: &str, viewport: u32) -> bool {
    let c = cond.trim().to_ascii_lowercase();
    if c.is_empty() {
        return true;
    }
    c.split("and").all(|parte| {
        let p = parte.trim();
        match p {
            "all" | "screen" => true,
            _ => largura_ok(p, viewport).unwrap_or(false),
        }
    })
}

/// `Some(verdadeiro)` se `(min-width: Npx)` ou `(max-width: Npx)` casa; `None` se a condição não é essa.
fn largura_ok(p: &str, viewport: u32) -> Option<bool> {
    let interno = p.strip_prefix('(')?.strip_suffix(')')?;
    let (nome, valor) = interno.split_once(':')?;
    let px: u32 = valor.trim().strip_suffix("px")?.trim().parse().ok()?;
    match nome.trim() {
        "min-width" => Some(viewport >= px),
        "max-width" => Some(viewport <= px),
        _ => None,
    }
}

/// Declarações de um bloco, respeitando `max_decls` e `max_value_len`.
fn declaracoes(s: &str, lim: &Limits) -> Vec<Declaration> {
    let mut out = Vec::new();
    for parte in Partes::new(s) {
        if out.len() >= lim.max_decls {
            break;
        }
        let Some((nome, valor)) = parte.split_once(':') else {
            continue;
        };
        let nome = nome.trim().to_ascii_lowercase();
        let valor = sem_important(valor.trim());
        if nome.is_empty() || valor.is_empty() || valor.len() > lim.max_value_len {
            continue;
        }
        out.push(Declaration {
            name: nome,
            value: valor.to_string(),
        });
    }
    out
}

fn sem_important(v: &str) -> &str {
    let v = v.trim_end();
    let n = v.len();
    let corte = n.saturating_sub(10);
    if n >= 10 && v.get(corte..).is_some_and(|t| t.eq_ignore_ascii_case("!important")) {
        v.get(..corte).unwrap_or("").trim_end()
    } else {
        v
    }
}

/// Separa declarações em `;` de nível zero, sem cortar dentro de parênteses ou aspas.
/// Preguiçoso: não aloca um vetor proporcional ao tamanho do texto.
struct Partes<'a> {
    s: &'a str,
    pos: usize,
    fim: bool,
}

impl<'a> Partes<'a> {
    fn new(s: &'a str) -> Self {
        Partes {
            s,
            pos: 0,
            fim: false,
        }
    }
}

impl<'a> Iterator for Partes<'a> {
    type Item = &'a str;

    fn next(&mut self) -> Option<&'a str> {
        if self.fim {
            return None;
        }
        let resto = self.s.get(self.pos..).unwrap_or("");
        let mut prof = 0usize;
        let mut aspas: Option<char> = None;
        for (k, c) in resto.char_indices() {
            match aspas {
                Some(q) => {
                    if c == q {
                        aspas = None;
                    }
                }
                None => match c {
                    '"' | '\'' => aspas = Some(c),
                    '(' => prof = prof.saturating_add(1),
                    ')' => prof = prof.saturating_sub(1),
                    ';' if prof == 0 => {
                        self.pos = self.pos.saturating_add(k).saturating_add(1);
                        return resto.get(..k);
                    }
                    _ => {}
                },
            }
        }
        self.fim = true;
        Some(resto)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::selector::Key;

    fn folha(css: &str, px: u32) -> Sheet {
        parse_sheet(css, px, &Limits::default())
    }

    #[test]
    fn regra_basica() {
        let s = folha(".a{color:red;margin:0}", 800);
        assert_eq!(s.rules.len(), 1);
        assert_eq!(s.rules[0].decls.len(), 2);
        assert_eq!(s.rules[0].decls[0].name, "color");
        assert_eq!(s.rules[0].decls[0].value, "red");
    }

    #[test]
    fn virgula_vira_varias_regras() {
        let s = folha("h1, h2 {color:red}", 800);
        assert_eq!(s.rules.len(), 2);
        assert_eq!(s.rules[0].selector.key(), Key::Tag("h1".into()));
        assert_eq!(s.rules[1].selector.key(), Key::Tag("h2".into()));
    }

    #[test]
    fn comentarios_e_important() {
        let s = folha("/*x*/a{color:red !important}", 800);
        assert_eq!(s.rules.len(), 1);
        assert_eq!(s.rules[0].decls[0].value, "red");
    }

    #[test]
    fn at_rules_ignoradas() {
        let s = folha(
            "@import url(x); @font-face{font-family:x} @keyframes k{from{a:b}} .x{color:red}",
            800,
        );
        assert_eq!(s.rules.len(), 1);
        assert_eq!(s.rules[0].selector.key(), Key::Class("x".into()));
    }

    #[test]
    fn media_min_width() {
        let css = "@media (min-width: 600px){.x{color:red}}";
        assert_eq!(folha(css, 800).rules.len(), 1);
        assert_eq!(folha(css, 400).rules.len(), 0);
    }

    #[test]
    fn media_max_width() {
        let css = "@media (max-width: 500px){.x{color:red}}";
        assert_eq!(folha(css, 400).rules.len(), 1);
        assert_eq!(folha(css, 800).rules.len(), 0);
    }

    #[test]
    fn media_print_ignorada() {
        assert_eq!(folha("@media print{.x{color:red}}", 800).rules.len(), 0);
    }

    #[test]
    fn media_aninhada_ignorada() {
        let css = "@media screen{@media (min-width: 1px){.x{color:red}}}";
        assert_eq!(folha(css, 800).rules.len(), 0);
    }

    #[test]
    fn ponto_e_virgula_em_url_e_aspas() {
        let d = parse_declarations("background:url(data:image/png;base64,AAAA);color:red");
        assert_eq!(d.len(), 2);
        let d = parse_declarations("content:\"a;b\";color:red");
        assert_eq!(d.len(), 2);
        assert_eq!(d[0].value, "\"a;b\"");
    }

    #[test]
    fn tetos() {
        let dez = "a{color:red}".repeat(10);
        let lim = Limits {
            max_rules: 3,
            ..Default::default()
        };
        assert_eq!(parse_sheet(&dez, 800, &lim).rules.len(), 3);

        let lim = Limits {
            max_value_len: 5,
            ..Default::default()
        };
        let s = parse_sheet("a{color:aaaaaaaaaa}", 800, &lim);
        assert!(s.rules.iter().all(|r| r.decls.is_empty()));

        let lim = Limits {
            max_decls: 2,
            ..Default::default()
        };
        let s = parse_sheet("a{a:1;b:2;c:3}", 800, &lim);
        assert_eq!(s.rules[0].decls.len(), 2);

        let lim = Limits {
            max_css_bytes: 4,
            ..Default::default()
        };
        let s = parse_sheet(".a{}.b{color:blue}", 800, &lim);
        assert_eq!(s.rules.len(), 1);
        assert_eq!(s.rules[0].selector.key(), Key::Class("a".into()));
    }

    #[test]
    fn truncado_nao_entra_em_panico() {
        assert_eq!(folha(".a{color:red", 800).rules.len(), 1);
        assert_eq!(folha("}}}", 800).rules.len(), 0);
        let _ = folha("@media (min-width:1px){.a{color:red}", 800);
        let _ = folha("/* sem fim", 800);
    }

    #[test]
    fn variaveis_sao_declaracoes() {
        let s = folha(":root{--bg:#000}", 800);
        assert_eq!(s.rules[0].decls[0].name, "--bg");
    }
}
