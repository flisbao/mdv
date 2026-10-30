//! Seletores CSS mínimos: tag, `*`, classe, id, atributos, estruturais, `:not`, `:root`
//! e os combinadores descendente, `>`, `+` e `~`. Pseudoclasses dinâmicas são descartadas.

use scraper::ElementRef;
use std::cell::Cell;

/// Máximo de compostos por seletor (a recursão de `casa` cresce com isso).
const MAX_PARTES: usize = 32;
/// Máximo de nós irmãos percorridos por `:nth-child`; acima disso a pseudoclasse não casa.
const MAX_IRMAOS: usize = 1024;
/// Passos de casamento por seletor e elemento: teto contra combinadores em cascata.
const MAX_PASSOS: u32 = 100_000;

/// Pseudoelemento aceito: `::before` ou `::after`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pseudo {
    Before,
    After,
}

/// Chave do último composto, usada pelo índice: id > primeira classe > tag > qualquer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Key {
    Id(String),
    Class(String),
    Tag(String),
    Any,
}

/// Combinador que liga um composto ao anterior. O primeiro composto não tem.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Comb { Nenhum, Descendente, Filho, Adjacente, Geral }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Operador { Existe, Igual, Palavra, Prefixo, Sufixo, Contem }

#[derive(Clone, Debug, PartialEq, Eq)]
struct Atributo {
    nome: String,
    op: Operador,
    valor: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Nth { Impar, Par, Posicao(i64) }

/// Uma condição simples dentro de um composto.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Simples {
    Tag(String),
    Id(String),
    Classe(String),
    Atributo(Atributo),
    PrimeiroFilho,
    UltimoFilho,
    FilhoUnico,
    NthFilho(Nth),
    Nao(Box<Simples>),
}

#[derive(Clone, Debug, PartialEq, Eq, Default)]
struct Composto {
    simples: Vec<Simples>,
}

/// Seletor complexo: compostos da esquerda para a direita, cada um com o combinador que o liga ao anterior.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Selector {
    partes: Vec<(Comb, Composto)>,
}

/// Interpreta um seletor. `None` se for inválido ou usar pseudoclasse dinâmica.
pub fn parse(s: &str) -> Option<Selector> {
    Leitor::new(s, 0).seletor()
}

/// Interpreta `x::before`, `x:after` etc. Sufixo sozinho vira `*`.
pub fn parse_pseudo(s: &str) -> Option<(Selector, Pseudo)> {
    let t = s.trim();
    let baixo = t.to_ascii_lowercase();
    let (sufixo, p) = [
        ("::before", Pseudo::Before),
        (":before", Pseudo::Before),
        ("::after", Pseudo::After),
        (":after", Pseudo::After),
    ]
    .into_iter()
    .find(|(suf, _)| baixo.ends_with(suf))?;
    let bruto = t.get(..t.len().saturating_sub(sufixo.len()))?;
    let base = bruto.trim();
    let sel = if base.is_empty() {
        parse("*")?
    } else if bruto.ends_with(char::is_whitespace) {
        parse(&format!("{base} *"))?
    } else {
        parse(base)?
    };
    Some((sel, p))
}

impl Selector {
    /// Especificidade (ids, classes/atributos/pseudoclasses, tags).
    pub fn specificity(&self) -> (u32, u32, u32) {
        self.partes
            .iter()
            .flat_map(|(_, c)| c.simples.iter())
            .fold((0, 0, 0), |(a, b, c), s| {
                let (x, y, z) = especificidade_simples(s);
                (a.saturating_add(x), b.saturating_add(y), c.saturating_add(z))
            })
    }

    /// Verdadeiro se o elemento casa com o seletor inteiro.
    pub fn matches(&self, el: ElementRef<'_>) -> bool {
        let passos = Cell::new(MAX_PASSOS);
        casa(&self.partes, el, &passos)
    }

    /// Chave do último composto para o índice.
    pub fn key(&self) -> Key {
        let Some((_, ultimo)) = self.partes.last() else {
            return Key::Any;
        };
        let id = ultimo.simples.iter().find_map(|s| match s {
            Simples::Id(i) => Some(Key::Id(i.clone())),
            _ => None,
        });
        let classe = ultimo.simples.iter().find_map(|s| match s {
            Simples::Classe(c) => Some(Key::Class(c.clone())),
            _ => None,
        });
        let tag = ultimo.simples.iter().find_map(|s| match s {
            Simples::Tag(t) => Some(Key::Tag(t.clone())),
            _ => None,
        });
        id.or(classe).or(tag).unwrap_or(Key::Any)
    }
}

fn especificidade_simples(s: &Simples) -> (u32, u32, u32) {
    match s {
        Simples::Id(_) => (1, 0, 0),
        Simples::Classe(_) | Simples::Atributo(_) | Simples::PrimeiroFilho | Simples::UltimoFilho => (0, 1, 0),
        Simples::FilhoUnico | Simples::NthFilho(_) => (0, 1, 0),
        Simples::Tag(_) => (0, 0, 1),
        Simples::Nao(i) => especificidade_simples(i),
    }
}

/// Casa a cadeia de compostos com `el` como último elemento, percorrendo os combinadores para trás.
fn casa(partes: &[(Comb, Composto)], el: ElementRef<'_>, passos: &Cell<u32>) -> bool {
    let Some(n) = passos.get().checked_sub(1) else {
        return false;
    };
    passos.set(n);
    let Some(((comb, c), resto)) = partes.split_last() else {
        return true;
    };
    if !c.casa(el) {
        return false;
    }
    if resto.is_empty() {
        return true;
    }
    match comb {
        Comb::Filho => el
            .parent()
            .and_then(ElementRef::wrap)
            .is_some_and(|p| casa(resto, p, passos)),
        Comb::Descendente => el
            .ancestors()
            .filter_map(ElementRef::wrap)
            .any(|a| casa(resto, a, passos)),
        Comb::Adjacente => el
            .prev_siblings()
            .find_map(ElementRef::wrap)
            .is_some_and(|s| casa(resto, s, passos)),
        Comb::Geral => el
            .prev_siblings()
            .filter_map(ElementRef::wrap)
            .any(|s| casa(resto, s, passos)),
        Comb::Nenhum => false,
    }
}

impl Composto {
    fn casa(&self, el: ElementRef<'_>) -> bool {
        self.simples.iter().all(|s| casa_simples(s, el))
    }
}

fn casa_simples(s: &Simples, el: ElementRef<'_>) -> bool {
    let v = el.value();
    match s {
        Simples::Tag(t) => v.name() == t.as_str(),
        Simples::Id(i) => v.id() == Some(i.as_str()),
        Simples::Classe(c) => v.classes().any(|x| x == c.as_str()),
        Simples::Atributo(a) => a.casa(v),
        Simples::PrimeiroFilho => el.prev_siblings().find_map(ElementRef::wrap).is_none(),
        Simples::UltimoFilho => el.next_siblings().find_map(ElementRef::wrap).is_none(),
        Simples::FilhoUnico => {
            el.prev_siblings().find_map(ElementRef::wrap).is_none()
                && el.next_siblings().find_map(ElementRef::wrap).is_none()
        }
        Simples::NthFilho(n) => match (n, posicao(el)) {
            (Nth::Impar, Some(p)) => p % 2 == 1,
            (Nth::Par, Some(p)) => p % 2 == 0,
            (Nth::Posicao(q), Some(p)) => i64::try_from(p).is_ok_and(|p| p == *q),
            (_, None) => false,
        },
        Simples::Nao(i) => !casa_simples(i, el),
    }
}

/// Posição (a partir de 1) entre os irmãos-elemento; `None` se houver irmãos demais para varrer.
fn posicao(el: ElementRef<'_>) -> Option<usize> {
    let mut antes = 0usize;
    let mut visitados = 0usize;
    for s in el.prev_siblings() {
        visitados += 1;
        if visitados > MAX_IRMAOS {
            return None;
        }
        if ElementRef::wrap(s).is_some() {
            antes += 1;
        }
    }
    Some(antes + 1)
}

impl Atributo {
    fn casa(&self, v: &scraper::node::Element) -> bool {
        let Some(atual) = v.attr(&self.nome) else {
            return false;
        };
        let vazio = self.valor.is_empty();
        match self.op {
            Operador::Existe => true,
            Operador::Igual => atual == self.valor,
            Operador::Palavra => {
                !vazio
                    && !self.valor.contains(char::is_whitespace)
                    && atual.split_whitespace().any(|p| p == self.valor)
            }
            Operador::Prefixo => !vazio && atual.starts_with(self.valor.as_str()),
            Operador::Sufixo => !vazio && atual.ends_with(self.valor.as_str()),
            Operador::Contem => !vazio && atual.contains(self.valor.as_str()),
        }
    }
}

/// Leitor de um seletor. `profundidade` impede `:not` dentro de `:not`.
struct Leitor {
    cs: Vec<char>,
    pos: usize,
    profundidade: u8,
}

fn eh_ident(c: char) -> bool {
    c.is_alphanumeric() || c == '-' || c == '_' || !c.is_ascii()
}

impl Leitor {
    fn new(s: &str, profundidade: u8) -> Self {
        Leitor {
            cs: s.chars().collect(),
            pos: 0,
            profundidade,
        }
    }

    fn peek(&self) -> Option<char> {
        self.cs.get(self.pos).copied()
    }

    fn avanca(&mut self) {
        self.pos = self.pos.saturating_add(1);
    }

    fn fim(&self) -> bool {
        self.pos >= self.cs.len()
    }

    /// Consome espaços; diz se consumiu algum.
    fn espacos(&mut self) -> bool {
        let ini = self.pos;
        while self.peek().is_some_and(char::is_whitespace) {
            self.avanca();
        }
        self.pos > ini
    }

    fn texto(&self, ini: usize, fim: usize) -> String {
        self.cs.get(ini..fim).map(|s| s.iter().collect()).unwrap_or_default()
    }

    fn ident(&mut self) -> Option<String> {
        let ini = self.pos;
        while self.peek().is_some_and(eh_ident) {
            self.avanca();
        }
        if self.pos == ini {
            return None;
        }
        Some(self.texto(ini, self.pos))
    }

    fn seletor(&mut self) -> Option<Selector> {
        self.espacos();
        let mut partes = Vec::new();
        let mut comb = Comb::Nenhum;
        loop {
            let composto = self.composto()?;
            partes.push((comb, composto));
            if partes.len() > MAX_PARTES {
                return None;
            }
            let com_espaco = self.espacos();
            match self.peek() {
                None => break,
                Some('>') => comb = Comb::Filho,
                Some('+') => comb = Comb::Adjacente,
                Some('~') => comb = Comb::Geral,
                Some(_) if com_espaco => comb = Comb::Descendente,
                Some(_) => return None,
            }
            if matches!(self.peek(), Some('>' | '+' | '~')) {
                self.avanca();
            }
            self.espacos();
            if self.fim() {
                return None;
            }
        }
        Some(Selector { partes })
    }

    fn composto(&mut self) -> Option<Composto> {
        let mut simples = Vec::new();
        let mut tokens = 0usize;
        loop {
            match self.peek() {
                Some('*') if tokens == 0 => {
                    self.avanca();
                    tokens += 1;
                }
                Some(c) if eh_ident(c) && tokens == 0 => {
                    let nome = self.ident()?.to_ascii_lowercase();
                    simples.push(Simples::Tag(nome));
                    tokens += 1;
                }
                Some('.') => {
                    self.avanca();
                    simples.push(Simples::Classe(self.ident()?));
                    tokens += 1;
                }
                Some('#') => {
                    self.avanca();
                    simples.push(Simples::Id(self.ident()?));
                    tokens += 1;
                }
                Some('[') => {
                    self.avanca();
                    simples.push(Simples::Atributo(self.atributo()?));
                    tokens += 1;
                }
                Some(':') => {
                    self.avanca();
                    simples.push(self.pseudo()?);
                    tokens += 1;
                }
                _ => break,
            }
        }
        if tokens == 0 {
            return None;
        }
        Some(Composto { simples })
    }

    /// Lê `[nome]`, `[nome=valor]` etc. Depois do `[`.
    fn atributo(&mut self) -> Option<Atributo> {
        self.espacos();
        let nome = self.ident()?.to_ascii_lowercase();
        self.espacos();
        let op = match self.peek()? {
            ']' => {
                self.avanca();
                return Some(Atributo {
                    nome,
                    op: Operador::Existe,
                    valor: String::new(),
                });
            }
            '=' => {
                self.avanca();
                Operador::Igual
            }
            c @ ('~' | '^' | '$' | '*') => {
                self.avanca();
                if self.peek()? != '=' {
                    return None;
                }
                self.avanca();
                match c {
                    '~' => Operador::Palavra,
                    '^' => Operador::Prefixo,
                    '$' => Operador::Sufixo,
                    _ => Operador::Contem,
                }
            }
            _ => return None,
        };
        self.espacos();
        let valor = match self.peek()? {
            q @ ('"' | '\'') => {
                self.avanca();
                let ini = self.pos;
                while self.peek().is_some_and(|c| c != q) {
                    self.avanca();
                }
                if self.peek() != Some(q) {
                    return None;
                }
                let v = self.texto(ini, self.pos);
                self.avanca();
                v
            }
            _ => {
                let ini = self.pos;
                while self.peek().is_some_and(|c| !c.is_whitespace() && c != ']') {
                    self.avanca();
                }
                if self.pos == ini {
                    return None;
                }
                self.texto(ini, self.pos)
            }
        };
        self.espacos();
        if self.peek() != Some(']') {
            return None;
        }
        self.avanca();
        Some(Atributo { nome, op, valor })
    }

    /// Lê a pseudoclasse depois do `:`.
    fn pseudo(&mut self) -> Option<Simples> {
        if self.peek() == Some(':') {
            return None;
        }
        let nome = self.ident()?.to_ascii_lowercase();
        let arg = if self.peek() == Some('(') {
            self.avanca();
            let ini = self.pos;
            while self.peek().is_some_and(|c| c != ')') {
                self.avanca();
            }
            if self.peek() != Some(')') {
                return None;
            }
            let a = self.texto(ini, self.pos);
            self.avanca();
            Some(a)
        } else {
            None
        };
        match (nome.as_str(), arg) {
            ("first-child", None) => Some(Simples::PrimeiroFilho),
            ("last-child", None) => Some(Simples::UltimoFilho),
            ("only-child", None) => Some(Simples::FilhoUnico),
            ("root", None) => Some(Simples::Tag("html".to_string())),
            ("nth-child", Some(a)) => Some(Simples::NthFilho(nth(&a)?)),
            ("not", Some(a)) if self.profundidade == 0 => {
                let mut sub = Leitor::new(&a, self.profundidade.saturating_add(1));
                sub.espacos();
                let mut c = sub.composto()?;
                sub.espacos();
                if !sub.fim() || c.simples.len() != 1 {
                    return None;
                }
                Some(Simples::Nao(Box::new(c.simples.pop()?)))
            }
            _ => None,
        }
    }
}

/// Argumento de `:nth-child`: `odd`, `even` ou um inteiro.
fn nth(a: &str) -> Option<Nth> {
    match a.trim().to_ascii_lowercase().as_str() {
        "odd" => Some(Nth::Impar),
        "even" => Some(Nth::Par),
        n => n.parse::<i64>().ok().map(Nth::Posicao),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use scraper::Html;

    /// Todos os elementos que casam com o seletor CSS de teste, em ordem de documento.
    fn nos<'a>(doc: &'a Html, css: &str) -> Vec<ElementRef<'a>> {
        doc.select(&scraper::Selector::parse(css).unwrap()).collect()
    }

    #[test]
    fn tag_classe_id() {
        let doc = Html::parse_document(r#"<div class="a b" id="x"></div><div class="b"></div>"#);
        let e = nos(&doc, "div");
        assert!(parse("div.a#x").unwrap().matches(e[0]));
        assert!(!parse("div.a#x").unwrap().matches(e[1]));
        assert!(parse("div.a").unwrap().matches(e[0]));
        assert!(!parse("span").unwrap().matches(e[0]));
    }

    #[test]
    fn descendente_e_filho() {
        let doc = Html::parse_document(r#"<div class="a"><section><p>t</p></section></div>"#);
        let p = nos(&doc, "p")[0];
        assert!(parse(".a p").unwrap().matches(p));
        assert!(!parse(".a > p").unwrap().matches(p));
        assert!(parse(".a > section > p").unwrap().matches(p));
    }

    #[test]
    fn irmao_adjacente_e_geral() {
        let doc = Html::parse_document("<h1>t</h1><p>1</p><p>2</p>");
        let ps = nos(&doc, "p");
        assert!(parse("h1 + p").unwrap().matches(ps[0]));
        assert!(!parse("h1 + p").unwrap().matches(ps[1]));
        assert!(parse("h1 ~ p").unwrap().matches(ps[0]));
        assert!(parse("h1 ~ p").unwrap().matches(ps[1]));
    }

    #[test]
    fn atributos() {
        let doc = Html::parse_document(r#"<a href="http://x.org" data-x="1" class="col-6">1</a><a data-x=2>2</a>"#);
        let a = nos(&doc, "a");
        assert!(parse("[data-x]").unwrap().matches(a[1]));
        assert!(parse("[data-x=1]").unwrap().matches(a[0]));
        assert!(!parse("[data-x=1]").unwrap().matches(a[1]));
        assert!(parse("[data-x=\"1\"]").unwrap().matches(a[0]));
        assert!(parse("[href^=\"http\"]").unwrap().matches(a[0]));
        assert!(!parse("[href^=\"http\"]").unwrap().matches(a[1]));
        assert!(parse("[href$=\".org\"]").unwrap().matches(a[0]));
        assert!(parse("[class*=col]").unwrap().matches(a[0]));
        assert!(!parse("[class*=col]").unwrap().matches(a[1]));
        assert!(parse("[class~=col-6]").unwrap().matches(a[0]));
    }

    #[test]
    fn first_last_nth() {
        let doc = Html::parse_document("<ul><li>1</li><li>2</li><li>3</li></ul>");
        let li = nos(&doc, "li");
        assert!(parse("li:first-child").unwrap().matches(li[0]));
        assert!(!parse("li:first-child").unwrap().matches(li[1]));
        assert!(parse("li:last-child").unwrap().matches(li[2]));
        assert!(!parse("li:last-child").unwrap().matches(li[1]));
        assert!(parse("li:nth-child(2)").unwrap().matches(li[1]));
        assert!(!parse("li:nth-child(2)").unwrap().matches(li[0]));
        assert!(parse("li:nth-child(odd)").unwrap().matches(li[0]));
        assert!(!parse("li:nth-child(odd)").unwrap().matches(li[1]));
        assert!(parse("li:nth-child(even)").unwrap().matches(li[1]));
        let unico = Html::parse_document("<ul><li>x</li></ul>");
        assert!(parse("li:only-child").unwrap().matches(nos(&unico, "li")[0]));
        assert!(!parse("li:only-child").unwrap().matches(li[0]));
    }

    #[test]
    fn not_simples() {
        let doc = Html::parse_document(r#"<p class="x">1</p><p>2</p>"#);
        let p = nos(&doc, "p");
        assert!(!parse("p:not(.x)").unwrap().matches(p[0]));
        assert!(parse("p:not(.x)").unwrap().matches(p[1]));
    }

    #[test]
    fn root() {
        let doc = Html::parse_document("<html><body><p>t</p></body></html>");
        let html = nos(&doc, "html")[0];
        let body = nos(&doc, "body")[0];
        assert!(parse(":root").unwrap().matches(html));
        assert!(!parse(":root").unwrap().matches(body));
    }

    #[test]
    fn especificidade() {
        assert_eq!(parse("#a .b c").unwrap().specificity(), (1, 1, 1));
        assert_eq!(parse("*").unwrap().specificity(), (0, 0, 0));
        assert_eq!(parse("ul li.x:first-child").unwrap().specificity(), (0, 2, 2));
    }

    #[test]
    fn chave_do_ultimo_composto() {
        assert_eq!(parse("#a .b c").unwrap().key(), Key::Tag("c".into()));
        assert_eq!(parse(".b c#a").unwrap().key(), Key::Id("a".into()));
        assert_eq!(parse(".b c").unwrap().key(), Key::Tag("c".into()));
        assert_eq!(parse(".b").unwrap().key(), Key::Class("b".into()));
        assert_eq!(parse("*").unwrap().key(), Key::Any);
    }

    #[test]
    fn dinamicos_sao_descartados() {
        assert!(parse("a:hover").is_none());
        assert!(parse("a:focus").is_none());
        assert!(parse("a:visited").is_none());
        assert!(parse("a::before").is_none());
        assert!(matches!(parse_pseudo("a::before"), Some((_, Pseudo::Before))));
        assert!(matches!(parse_pseudo("a:after"), Some((_, Pseudo::After))));
        assert!(matches!(parse_pseudo("::after"), Some((_, Pseudo::After))));
        assert!(parse_pseudo("a").is_none());
        assert!(parse_pseudo("a:hover").is_none());
    }

    #[test]
    fn invalidos() {
        assert!(parse("").is_none());
        assert!(parse("div >").is_none());
        assert!(parse("[x").is_none());
        assert!(parse("$$").is_none());
    }

    #[test]
    fn partes_demais_sao_recusadas() {
        assert!(parse(&"a ".repeat(40)).is_none());
    }
}
