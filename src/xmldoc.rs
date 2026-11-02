//! Leitura de XML para `serde_json::Value` sobre os eventos do `quick-xml`, com tetos de nós,
//! profundidade e texto, e erro de sintaxe localizado com mensagem em português.
//!
//! Mapeamento: a raiz não vira chave (`XmlDoc.value` é o conteúdo dela); nomes ficam sem prefixo de
//! namespace e `xmlns` some; atributo vira `@nome`; texto de elemento com filhos ou atributos vai
//! para `#text`; filho repetido vira lista. Sem inferência de tipo: `00123` continua texto.
//! `<!DOCTYPE>` é recusado (sem DTD nem entidades externas: só as cinco predefinidas e as numéricas).

use quick_xml::errors::{Error as QxError, IllFormedError, SyntaxError as QxSyntax};
use quick_xml::escape::EscapeError;
use quick_xml::events::attributes::AttrError;
use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;
use serde_json::{Map, Value};

use crate::mem;
use crate::render::sanitize;
use crate::syntax_error::SyntaxError;

/// Tetos da conversão: nós criados (elementos, atributos e `#text`), elementos abertos ao mesmo
/// tempo e bytes de nomes e textos.
#[derive(Debug, Clone)]
pub struct Limits {
    pub max_nodes: usize,
    pub max_depth: usize,
    pub max_string_bytes: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Limits { max_nodes: 1_000_000, max_depth: 128, max_string_bytes: 32 << 20 }
    }
}

/// Documento lido: nome local da raiz (só para o resumo da vista) e o conteúdo dela.
#[derive(Debug)]
pub struct XmlDoc {
    pub root: String,
    pub value: Value,
}

#[derive(Debug)]
pub enum XmlError {
    /// Erro de sintaxe (ou DOCTYPE) com posição no arquivo.
    Syntax(SyntaxError),
    /// Algum teto de `Limits` (ou da memória do processo) estourou.
    Limit(String),
    /// Nenhum elemento (texto vazio ou só comentários).
    Empty,
}

const TOO_BIG: &str = "estrutura grande demais: use a vista de texto";
const TOO_DEEP: &str = "aninhamento fundo demais: use a vista de texto";

fn limit(msg: &str) -> XmlError {
    XmlError::Limit(msg.to_string())
}

/// Elemento aberto na pilha explícita (sem recursão).
struct Frame {
    name: String,
    /// Posição (em bytes) do `<` de abertura, para apontar o elemento que ficou sem fechar.
    start: usize,
    attrs: Map<String, Value>,
    children: Map<String, Value>,
    /// Trechos de texto (e CDATA) na ordem do documento, sem tratamento.
    texts: Vec<String>,
}

struct Builder<'l> {
    limits: &'l Limits,
    nodes: usize,
    bytes: usize,
    stack: Vec<Frame>,
    /// Elemento raiz já fechado.
    root: Option<(String, Value)>,
}

/// Lê `text` como um documento XML. A leitura para no primeiro erro ou teto estourado.
pub fn parse(text: &str, limits: &Limits) -> Result<XmlDoc, XmlError> {
    // Com BOM, as posições valem para o texto sem ele (a coluna 1 da linha 1 é depois do BOM).
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut reader = Reader::from_str(text);
    let cfg = reader.config_mut();
    cfg.check_end_names = true;
    cfg.expand_empty_elements = false;
    cfg.trim_text(false);
    let mut b = Builder { limits, nodes: 0, bytes: 0, stack: Vec::new(), root: None };
    loop {
        if mem::exceeded() {
            return Err(limit(TOO_BIG));
        }
        // Lido antes do evento: aponta o `<` do que vem (ou o começo do texto).
        let start = reader.buffer_position() as usize;
        let ev = match reader.read_event() {
            Ok(ev) => ev,
            Err(e) => {
                let at = match e {
                    QxError::Syntax(_) => reader.error_position() as usize,
                    _ => start,
                };
                return Err(syntax(text, at, traduz(&e)));
            }
        };
        match ev {
            Event::Start(e) => b.open(text, start, &e, false)?,
            Event::Empty(e) => b.open(text, start, &e, true)?,
            Event::End(_) => b.close()?,
            Event::Text(t) => match t.unescape() {
                Ok(s) => b.text(text, start, &s)?,
                Err(e) => return Err(syntax(text, start, traduz(&e))),
            },
            Event::CData(c) => match c.decode() {
                Ok(s) => b.text(text, start, &s)?,
                Err(_) => return Err(syntax(text, start, "XML malformado: texto com codificação inválida".into())),
            },
            Event::DocType(_) => {
                return Err(syntax(text, start, "declaração DOCTYPE não é aceita (segurança)".into()));
            }
            Event::Eof => break,
            Event::Comment(_) | Event::Decl(_) | Event::PI(_) => {}
        }
    }
    if let Some(f) = b.stack.last() {
        let msg = format!("elemento <{}> sem fechar", f.name);
        return Err(syntax(text, f.start, msg));
    }
    match b.root {
        Some((root, value)) => Ok(XmlDoc { root, value }),
        None => Err(XmlError::Empty),
    }
}

impl Builder<'_> {
    /// Soma nós e bytes e confere os tetos.
    fn charge(&mut self, nodes: usize, bytes: usize) -> Result<(), XmlError> {
        self.nodes += nodes;
        self.bytes += bytes;
        if self.nodes > self.limits.max_nodes || self.bytes > self.limits.max_string_bytes {
            return Err(limit(TOO_BIG));
        }
        Ok(())
    }

    /// `<x ...>` ou `<x .../>` (`empty`: fecha logo em seguida).
    fn open(&mut self, text: &str, start: usize, e: &BytesStart<'_>, empty: bool) -> Result<(), XmlError> {
        if self.stack.is_empty() && self.root.is_some() {
            return Err(syntax(text, start, "mais de um elemento raiz".into()));
        }
        // O filho que abre agora ocupa o nível `stack.len() + 1`.
        if self.stack.len() + 1 > self.limits.max_depth {
            return Err(limit(TOO_DEEP));
        }
        let name = local(e.name().as_ref());
        self.charge(1, name.len())?;
        let mut frame = Frame {
            name: sanitize(&name).into_owned(),
            start,
            attrs: Map::new(),
            children: Map::new(),
            texts: Vec::new(),
        };
        for attr in e.attributes() {
            let attr = attr.map_err(|err| syntax(text, start, traduz_attr(&err)))?;
            let key = std::str::from_utf8(attr.key.as_ref()).unwrap_or("");
            if key == "xmlns" || key.starts_with("xmlns:") {
                continue;
            }
            let value = attr.unescape_value().map_err(|err| syntax(text, start, traduz(&err)))?;
            let key = format!("@{}", sanitize(key.rsplit(':').next().unwrap_or(key)));
            self.charge(1, key.len() + value.len())?;
            // Nomes locais iguais de namespaces diferentes: a última vence.
            frame.attrs.insert(key, Value::String(sanitize(&value).into_owned()));
        }
        self.stack.push(frame);
        if empty {
            self.close()?;
        }
        Ok(())
    }

    /// Texto ou CDATA: fora da raiz só vale espaço.
    fn text(&mut self, text: &str, start: usize, s: &str) -> Result<(), XmlError> {
        let Some(top) = self.stack.last_mut() else {
            if s.trim().is_empty() {
                return Ok(());
            }
            return Err(syntax(text, start, "texto fora do elemento raiz".into()));
        };
        top.texts.push(sanitize(s).into_owned());
        self.charge(0, s.len())
    }

    /// Fecha o elemento do topo e o entrega ao pai (ou o guarda como raiz).
    fn close(&mut self) -> Result<(), XmlError> {
        let Some(f) = self.stack.pop() else {
            return Ok(());
        };
        let name = f.name.clone();
        let value = self.build(f)?;
        match self.stack.last_mut() {
            Some(parent) => add_child(&mut parent.children, name, value),
            None => self.root = Some((name, value)),
        }
        Ok(())
    }

    /// Valor do elemento: texto (ou null) se não tem atributos nem filhos; senão objeto com os
    /// atributos, `#text` e os filhos, nessa ordem.
    fn build(&mut self, mut f: Frame) -> Result<Value, XmlError> {
        if f.attrs.is_empty() && f.children.is_empty() {
            let joined = f.texts.concat();
            let t = joined.trim();
            return Ok(if t.is_empty() { Value::Null } else { Value::String(t.to_string()) });
        }
        let text: Vec<&str> = f.texts.iter().map(|t| t.trim()).filter(|t| !t.is_empty()).collect();
        let mut map = Map::new();
        map.append(&mut f.attrs);
        if !text.is_empty() {
            self.charge(1, 0)?;
            map.insert("#text".to_string(), Value::String(text.join(" ")));
        }
        map.append(&mut f.children);
        Ok(Value::Object(map))
    }
}

/// Filho repetido vira lista na posição da primeira ocorrência.
fn add_child(children: &mut Map<String, Value>, name: String, value: Value) {
    match children.get_mut(&name) {
        None => {
            children.insert(name, value);
        }
        // Um filho nunca é lista; lista aqui já é o agrupamento de irmãos repetidos.
        Some(Value::Array(items)) => items.push(value),
        Some(first) => {
            let first = std::mem::take(first);
            children.insert(name, Value::Array(vec![first, value]));
        }
    }
}

/// Nome local (depois do último `:`).
fn local(qname: &[u8]) -> String {
    let name = String::from_utf8_lossy(qname);
    name.rsplit(':').next().unwrap_or(&name).to_string()
}

fn syntax(text: &str, byte: usize, message: String) -> XmlError {
    let (line, col) = line_col(text, byte);
    XmlError::Syntax(SyntaxError { line, col, message })
}

/// Linha e coluna (em caracteres) 1-based de `byte`, limitado ao fim e a uma fronteira de caractere.
fn line_col(text: &str, byte: usize) -> (usize, usize) {
    let mut at = byte.min(text.len());
    while !text.is_char_boundary(at) {
        at -= 1;
    }
    let before = &text[..at];
    let line = before.matches('\n').count() + 1;
    let col = before.rsplit('\n').next().map_or(0, |l| l.chars().count()) + 1;
    (line, col)
}

/// Mensagem do `quick-xml` em português, numa linha só; nomes vindos do arquivo passam por `sanitize`.
fn traduz(e: &QxError) -> String {
    let name = |n: &str| sanitize(n).into_owned();
    match e {
        QxError::IllFormed(IllFormedError::MismatchedEndTag { expected, found }) => {
            format!("esperava </{}> mas veio </{}>", name(expected), name(found))
        }
        QxError::IllFormed(IllFormedError::UnmatchedEndTag(n)) => {
            format!("</{}> sem abertura correspondente", name(n))
        }
        QxError::IllFormed(IllFormedError::DoubleHyphenInComment) => "`--` não pode aparecer dentro de comentário".into(),
        QxError::Syntax(QxSyntax::UnclosedTag) => "tag sem fechar (falta `>`)".into(),
        QxError::Syntax(QxSyntax::UnclosedComment) => "comentário sem fechar (falta `-->`)".into(),
        QxError::Syntax(QxSyntax::UnclosedCData) => "CDATA sem fechar (falta `]]>`)".into(),
        QxError::Syntax(QxSyntax::UnclosedPIOrXmlDecl) => "instrução de processamento sem fechar (falta `?>`)".into(),
        QxError::Syntax(QxSyntax::UnclosedDoctype) => "declaração DOCTYPE não é aceita (segurança)".into(),
        QxError::Escape(EscapeError::UnrecognizedEntity(_, n)) => {
            format!("entidade desconhecida `&{};` (só valem &lt; &gt; &amp; &quot; &apos; e as numéricas)", name(n))
        }
        QxError::Escape(EscapeError::UnterminatedEntity(_)) => "`&` sem `;` (use &amp; para o caractere &)".into(),
        QxError::Escape(EscapeError::InvalidCharRef(_)) => "referência numérica de caractere inválida".into(),
        QxError::InvalidAttr(a) => traduz_attr(a),
        _ => "XML malformado".into(),
    }
}

fn traduz_attr(e: &AttrError) -> String {
    match e {
        AttrError::Duplicated(..) => "atributo repetido no mesmo elemento".into(),
        AttrError::ExpectedEq(_) => "atributo sem `=`".into(),
        AttrError::ExpectedValue(_) | AttrError::UnquotedValue(_) | AttrError::ExpectedQuote(..) => {
            "valor de atributo sem aspas".into()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ok(s: &str) -> XmlDoc {
        match parse(s, &Limits::default()) {
            Ok(d) => d,
            Err(e) => panic!("{s:?} deveria ler: {e:?}"),
        }
    }

    fn erro(s: &str) -> XmlError {
        match parse(s, &Limits::default()) {
            Err(e) => e,
            Ok(d) => panic!("{s:?} deveria falhar, deu {:?}", d.value),
        }
    }

    fn sintaxe(s: &str) -> SyntaxError {
        match erro(s) {
            XmlError::Syntax(e) => e,
            outro => panic!("{s:?} deveria dar erro de sintaxe: {outro:?}"),
        }
    }

    #[test]
    fn elemento_folha_vira_texto() {
        let d = ok("<a>oi</a>");
        assert_eq!(d.root, "a");
        assert_eq!(d.value, json!("oi"));
    }

    #[test]
    fn filhos_e_atributos_na_ordem() {
        let d = ok(r#"<p id="7"><nome>Ana</nome><idade>30</idade></p>"#);
        assert_eq!(d.value.to_string(), r#"{"@id":"7","nome":"Ana","idade":"30"}"#);
    }

    #[test]
    fn irmaos_repetidos_viram_lista() {
        let d = ok("<l><i>1</i><i>2</i><x/></l>");
        assert_eq!(d.value, json!({"i": ["1", "2"], "x": null}));
    }

    #[test]
    fn texto_misto_vai_para_hash_text() {
        let d = ok(r#"<p a="1">oi <b>x</b> fim</p>"#);
        assert_eq!(d.value, json!({"@a": "1", "#text": "oi fim", "b": "x"}));
    }

    #[test]
    fn cdata_vira_texto() {
        assert_eq!(ok("<a><![CDATA[<x>]]></a>").value, json!("<x>"));
    }

    #[test]
    fn namespaces_somem() {
        let d = ok(r#"<s:Env xmlns:s="u" xmlns="d" s:k="v"><s:Body/></s:Env>"#);
        assert_eq!(d.root, "Env");
        assert_eq!(d.value, json!({"@k": "v", "Body": null}));
    }

    #[test]
    fn entidades_predefinidas_e_numericas() {
        assert_eq!(ok("<a>&lt;&amp;&#233;</a>").value, json!("<&é"));
    }

    #[test]
    fn zeros_a_esquerda_ficam() {
        assert_eq!(ok("<c>00123</c>").value, json!("00123"));
        assert_eq!(ok("<c>12345678901234567890</c>").value, json!("12345678901234567890"));
    }

    #[test]
    fn espaco_entre_elementos_comentarios_e_declaracao_sao_ignorados() {
        let d = ok("<?xml version=\"1.0\"?>\n<!-- c -->\n<a>\n  <b>1</b>\n</a>\n");
        assert_eq!(d.value, json!({"b": "1"}));
    }

    #[test]
    fn controle_vira_substituto() {
        let t = ok("<a>\u{1b}[31mx</a>").value.to_string();
        assert!(t.contains('\u{fffd}') && !t.contains('\u{1b}'), "{t:?}");
    }

    #[test]
    fn doctype_e_recusado_com_a_linha() {
        let e = sintaxe("<?xml version=\"1.0\"?>\n<!DOCTYPE a [<!ENTITY e \"x\">]>\n<a>&e;</a>");
        assert_eq!(e.line, 2, "{e:?}");
        assert!(e.message.contains("DOCTYPE"), "{}", e.message);
    }

    #[test]
    fn entidade_desconhecida_e_erro() {
        let e = sintaxe("<a>&e;</a>");
        assert!(e.message.contains("entidade"), "{}", e.message);
    }

    #[test]
    fn fechamento_trocado_aponta_a_linha() {
        let e = sintaxe("<a>\n  <b>\n</a>");
        assert_eq!(e.line, 3, "{e:?}");
        assert!(e.message.contains("</b>") && e.message.contains("</a>"), "{}", e.message);
    }

    #[test]
    fn fim_com_elemento_aberto() {
        let e = sintaxe("<a><b>x</b>");
        assert!(e.message.contains("<a>"), "{}", e.message);
    }

    #[test]
    fn duas_raizes_e_erro() {
        let e = sintaxe("<a/><b/>");
        assert!(e.message.contains("raiz"), "{}", e.message);
    }

    #[test]
    fn texto_fora_da_raiz_e_erro() {
        let e = sintaxe("oi <a/>");
        assert!(e.message.contains("fora"), "{}", e.message);
    }

    #[test]
    fn vazio_ou_so_comentario() {
        assert!(matches!(erro(""), XmlError::Empty));
        assert!(matches!(erro("<!-- x -->"), XmlError::Empty));
    }

    #[test]
    fn limite_de_nos() {
        let l = Limits { max_nodes: 3, ..Limits::default() };
        assert!(matches!(parse("<a><b/><c/><d/></a>", &l), Err(XmlError::Limit(_))));
    }

    #[test]
    fn limite_de_bytes_de_texto() {
        let l = Limits { max_string_bytes: 4, ..Limits::default() };
        assert!(matches!(parse("<a>abcdefgh</a>", &l), Err(XmlError::Limit(_))));
    }

    #[test]
    fn limite_de_profundidade_pequeno() {
        let l = Limits { max_depth: 3, ..Limits::default() };
        match parse("<a><b><c><d/></c></b></a>", &l) {
            Err(XmlError::Limit(m)) => assert!(m.contains("aninhamento"), "{m}"),
            outro => panic!("{outro:?}"),
        }
    }

    #[test]
    fn bom_e_ignorado() {
        assert_eq!(ok("\u{feff}<a>x</a>").value, json!("x"));
    }

    #[test]
    fn atributo_repetido_e_sem_aspas_sao_erro() {
        assert!(sintaxe(r#"<a k="1" k="2"/>"#).message.contains("repetido"));
        assert!(sintaxe("<a k=1/>").message.contains("aspas"));
    }

    #[test]
    fn tag_sem_fechar_aponta_o_comeco() {
        let e = sintaxe("<a>\n<b");
        assert_eq!(e.line, 2, "{e:?}");
        assert!(e.message.contains("tag sem fechar"), "{}", e.message);
    }

    #[test]
    fn colunas_contam_caracteres() {
        let e = sintaxe("<a>é\u{e9}</b>");
        assert_eq!((e.line, e.col), (1, 6), "{e:?}");
    }

    #[test]
    fn cem_mil_aberturas_nao_estouram_a_pilha() {
        match parse(&"<a>".repeat(100_000), &Limits::default()) {
            Err(XmlError::Limit(m)) => assert!(m.contains("aninhamento"), "{m}"),
            outro => panic!("{:?}", outro.map(|d| d.root)),
        }
    }
}
