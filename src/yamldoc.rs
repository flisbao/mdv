//! Leitura de YAML para `serde_json::Value` sobre o iterador de eventos do `saphyr-parser`, com
//! tetos de nós, profundidade e texto, e erro de sintaxe localizado com mensagem em português.

use std::collections::HashMap;
use std::io::{self, Write};

use saphyr_parser::{Event, Parser, ScalarStyle, ScanError, Tag};
use serde_json::{Map, Number, Value};

use crate::mem;
use crate::syntax_error::SyntaxError;

/// Tamanho máximo, em caracteres, do texto de uma chave composta (mapa ou lista como chave).
pub const MAX_KEY_CHARS: usize = 200;

/// Tetos da conversão: nós (inclusive os clonados por alias), coleções aninhadas e bytes de texto.
#[derive(Debug, Clone)]
pub struct Limits {
    pub max_nodes: usize,
    pub max_depth: usize,
    pub max_string_bytes: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Limits { max_nodes: 200_000, max_depth: 128, max_string_bytes: 32 << 20 }
    }
}

#[derive(Debug)]
pub enum YamlError {
    /// Erro de sintaxe com posição no arquivo.
    Syntax(SyntaxError),
    /// Algum teto de `Limits` (ou da memória do processo) estourou.
    Limit(String),
    /// Nenhum documento (texto vazio ou só comentários).
    Empty,
}

const TOO_BIG: &str = "estrutura grande demais: use a vista de texto";
const TOO_DEEP: &str = "aninhamento fundo demais: use a vista de texto";

fn limit(msg: &str) -> YamlError {
    YamlError::Limit(msg.to_string())
}

/// Lê `text` (um ou mais documentos YAML) como `Value`: um documento vira o próprio valor, vários
/// viram uma lista. Os eventos são puxados um a um e a leitura para no primeiro teto estourado.
pub fn parse(text: &str, limits: &Limits) -> Result<Value, YamlError> {
    let mut b = Builder { limits, nodes: 0, bytes: 0, stack: Vec::new(), anchors: HashMap::new(), docs: Vec::new() };
    for ev in Parser::new_from_str(text) {
        if mem::exceeded() {
            return Err(limit(TOO_BIG));
        }
        match ev {
            Ok((ev, _)) => b.event(ev)?,
            Err(e) => return Err(scan_error(text, &e)),
        }
    }
    let mut docs = b.docs;
    match docs.len() {
        0 => Err(YamlError::Empty),
        1 => Ok(docs.swap_remove(0)),
        _ => Ok(Value::Array(docs)),
    }
}

/// Chave pendente de um mapa: texto, ou `<<` (merge key) esperando o mapa a mesclar.
enum Key {
    Text(String),
    Merge,
}

/// Coleção aberta na pilha explícita (sem recursão), com o id da âncora (0 = sem âncora).
enum Frame {
    Seq { items: Vec<Value>, anchor: usize },
    Map { map: Map<String, Value>, key: Option<Key>, anchor: usize },
}

struct Builder<'l> {
    limits: &'l Limits,
    nodes: usize,
    bytes: usize,
    stack: Vec<Frame>,
    /// Valor já montado de cada âncora do documento atual; o alias clona.
    anchors: HashMap<usize, Value>,
    docs: Vec<Value>,
}

impl Builder<'_> {
    fn event(&mut self, ev: Event<'_>) -> Result<(), YamlError> {
        match ev {
            Event::DocumentStart(_) => {
                self.stack.clear();
                self.anchors.clear();
            }
            Event::Scalar(s, style, anchor, tag) => {
                let plain = matches!(style, ScalarStyle::Plain);
                if plain && tag.is_none() && s == "<<" {
                    if let Some(Frame::Map { key: k @ None, .. }) = self.stack.last_mut() {
                        *k = Some(Key::Merge);
                        return Ok(());
                    }
                }
                self.charge(1, s.len())?;
                let v = scalar(&s, plain, tag.as_deref());
                self.finish(v, anchor)?;
            }
            Event::Alias(id) => {
                // Âncora ainda aberta (`&a [*a]`) não tem valor: vira null.
                let v = match self.anchors.get(&id) {
                    Some(v) => {
                        let (n, b) = size(v, self.room())?;
                        let v = v.clone();
                        self.charge(n, b)?;
                        v
                    }
                    None => {
                        self.charge(1, 0)?;
                        Value::Null
                    }
                };
                self.place(v);
            }
            Event::SequenceStart(anchor, _) => self.open(Frame::Seq { items: Vec::new(), anchor })?,
            Event::MappingStart(anchor, _) => self.open(Frame::Map { map: Map::new(), key: None, anchor })?,
            Event::SequenceEnd | Event::MappingEnd => {
                let (v, anchor) = match self.stack.pop() {
                    Some(Frame::Seq { items, anchor }) => (Value::Array(items), anchor),
                    Some(Frame::Map { map, anchor, .. }) => (Value::Object(map), anchor),
                    None => return Ok(()),
                };
                self.finish(v, anchor)?;
            }
            _ => {}
        }
        Ok(())
    }

    /// Nós e bytes que ainda cabem nos tetos.
    fn room(&self) -> (usize, usize) {
        (
            self.limits.max_nodes.saturating_sub(self.nodes),
            self.limits.max_string_bytes.saturating_sub(self.bytes),
        )
    }

    fn charge(&mut self, nodes: usize, bytes: usize) -> Result<(), YamlError> {
        self.nodes += nodes;
        self.bytes += bytes;
        if self.nodes > self.limits.max_nodes || self.bytes > self.limits.max_string_bytes {
            return Err(limit(TOO_BIG));
        }
        Ok(())
    }

    /// Abre uma coleção; passar de `max_depth` interrompe a leitura (o parser não avança mais).
    fn open(&mut self, frame: Frame) -> Result<(), YamlError> {
        self.charge(1, 0)?;
        self.stack.push(frame);
        if self.stack.len() > self.limits.max_depth {
            return Err(limit(TOO_DEEP));
        }
        Ok(())
    }

    /// Valor completo: guarda a cópia da âncora (que também conta nos tetos) e o põe no pai.
    fn finish(&mut self, v: Value, anchor: usize) -> Result<(), YamlError> {
        if anchor != 0 {
            let (n, b) = size(&v, self.room())?;
            self.charge(n, b)?;
            self.anchors.insert(anchor, v.clone());
        }
        self.place(v);
        Ok(())
    }

    fn place(&mut self, v: Value) {
        match self.stack.last_mut() {
            None => self.docs.push(v),
            Some(Frame::Seq { items, .. }) => items.push(v),
            Some(Frame::Map { map, key, .. }) => match key.take() {
                None => *key = Some(Key::Text(key_text(v))),
                Some(Key::Text(k)) => {
                    // Chave duplicada: a última vence.
                    map.insert(k, v);
                }
                Some(Key::Merge) => merge(map, v),
            },
        }
    }
}

/// Nós e bytes de texto de `v`, sem recursão; para assim que passa de `room` (antes de clonar).
fn size(v: &Value, room: (usize, usize)) -> Result<(usize, usize), YamlError> {
    let (mut nodes, mut bytes) = (0usize, 0usize);
    let mut pending = vec![v];
    while let Some(v) = pending.pop() {
        nodes += 1;
        match v {
            Value::String(s) => bytes += s.len(),
            Value::Array(items) => pending.extend(items),
            Value::Object(map) => {
                for (k, item) in map {
                    bytes += k.len();
                    pending.push(item);
                }
            }
            _ => {}
        }
        if nodes > room.0 || bytes > room.1 {
            return Err(limit(TOO_BIG));
        }
    }
    Ok((nodes, bytes))
}

/// `<<`: mescla um mapa (ou uma lista de mapas, em que o primeiro vence) sem sobrescrever as
/// chaves do próprio mapa; chave explícita que vier depois também vence. Outro valor é chave comum.
fn merge(map: &mut Map<String, Value>, v: Value) {
    let sources: Vec<Map<String, Value>> = match v {
        Value::Object(m) => vec![m],
        Value::Array(items) if items.iter().all(Value::is_object) => items
            .into_iter()
            .filter_map(|i| match i {
                Value::Object(m) => Some(m),
                _ => None,
            })
            .collect(),
        other => {
            map.insert("<<".to_string(), other);
            return;
        }
    };
    for source in sources {
        for (k, v) in source {
            map.entry(k).or_insert(v);
        }
    }
}

/// Chave como texto: número, bool e null pelo JSON; mapa ou lista pelo JSON compacto, cortado em
/// `MAX_KEY_CHARS` (o texto gerado também tem teto, não só o resultado).
fn key_text(v: Value) -> String {
    let Value::String(s) = v else {
        let mut out = Capped(Vec::new());
        let _ = serde_json::to_writer(&mut out, &v); // falha só ao bater o teto
        let text = String::from_utf8_lossy(&out.0);
        if text.chars().count() <= MAX_KEY_CHARS {
            return text.into_owned();
        }
        return text.chars().take(MAX_KEY_CHARS - 1).chain(['…']).collect();
    };
    s
}

/// Escritor com teto de bytes, para gerar o JSON da chave composta sem montar o texto inteiro.
struct Capped(Vec<u8>);

impl Write for Capped {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let room = (MAX_KEY_CHARS * 4 + 4).saturating_sub(self.0.len());
        if room == 0 {
            return Err(io::Error::other("chave longa demais"));
        }
        let n = buf.len().min(room);
        self.0.extend_from_slice(&buf[..n]);
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Escalar pelo esquema core do YAML 1.2. Entre aspas, bloco `|`/`>` ou com `!!str` é sempre
/// texto; as demais tags são ignoradas.
fn scalar(s: &str, plain: bool, tag: Option<&Tag>) -> Value {
    let str_tag = tag.is_some_and(|t| t.is_yaml_core_schema() && t.suffix == "str");
    if str_tag || !plain {
        return Value::String(s.to_string());
    }
    match s {
        "" | "~" | "null" | "Null" | "NULL" => Value::Null,
        "true" | "True" | "TRUE" => Value::Bool(true),
        "false" | "False" | "FALSE" => Value::Bool(false),
        _ => number(s).unwrap_or_else(|| Value::String(s.to_string())),
    }
}

/// Inteiro decimal, `0x`, `0o` ou float; `.inf`/`.nan` e o que não cabe em `f64` ficam texto.
fn number(s: &str) -> Option<Value> {
    if let Some(d) = s.strip_prefix("0x") {
        return radix(d, 16);
    }
    if let Some(d) = s.strip_prefix("0o") {
        return radix(d, 8);
    }
    let unsigned = s.strip_prefix(['-', '+']).unwrap_or(s);
    if !unsigned.is_empty() && unsigned.bytes().all(|c| c.is_ascii_digit()) {
        if let Ok(i) = s.parse::<i64>() {
            return Some(i.into());
        }
        if let Ok(u) = s.parse::<u64>() {
            return Some(u.into());
        }
    } else if !is_float(unsigned) {
        return None;
    }
    Number::from_f64(s.parse().ok()?).map(Value::Number)
}

fn radix(digits: &str, base: u32) -> Option<Value> {
    if digits.is_empty() || !digits.chars().all(|c| c.is_digit(base)) {
        return None;
    }
    u64::from_str_radix(digits, base).ok().map(Value::from)
}

/// Float do esquema core, sem o sinal: `(\.[0-9]+|[0-9]+(\.[0-9]*)?)([eE][-+]?[0-9]+)?`.
fn is_float(s: &str) -> bool {
    let digits = |t: &str| t.bytes().all(|c| c.is_ascii_digit());
    let (mantissa, exp) = match s.find(['e', 'E']) {
        Some(i) => (&s[..i], Some(&s[i + 1..])),
        None => (s, None),
    };
    let (int, frac) = match mantissa.split_once('.') {
        Some((a, b)) => (a, Some(b)),
        None => (mantissa, None),
    };
    let mantissa_ok = digits(int)
        && frac.is_none_or(digits)
        && (!int.is_empty() || frac.is_some_and(|f| !f.is_empty()));
    let exp_ok = exp.is_none_or(|e| {
        let e = e.strip_prefix(['-', '+']).unwrap_or(e);
        !e.is_empty() && digits(e)
    });
    mantissa_ok && exp_ok
}

/// `ScanError` com posição 1-based e mensagem em português. O teto de aninhamento do próprio
/// parser (255 coleções de fluxo) é limite, não erro de sintaxe.
fn scan_error(text: &str, e: &ScanError) -> YamlError {
    if e.info().contains("recursion limit exceeded") {
        return limit(TOO_DEEP);
    }
    let (line, col) = position(text, e.marker().line().max(1), e.marker().col());
    YamlError::Syntax(SyntaxError { line, col, message: message_pt(e.info()) })
}

/// Linha e coluna 1-based. O parser aponta o fim do arquivo como coluna 0 da linha seguinte ao
/// texto; nesse caso vale o fim da última linha com conteúdo, onde está o que ficou aberto.
fn position(text: &str, line: usize, col: usize) -> (usize, usize) {
    if col > 0 || line == 1 {
        return (line, col + 1);
    }
    let blank = |l: &str| l.trim_matches(|c: char| c.is_whitespace() || c == '\u{feff}').is_empty();
    let mut last = None;
    for (i, l) in text.split('\n').enumerate() {
        if i + 1 < line {
            if !blank(l) {
                last = Some((i, l));
            }
        } else if !blank(l) {
            return (line, 1);
        }
    }
    match last {
        Some((i, l)) => {
            let l = l.strip_suffix('\r').unwrap_or(l).trim_start_matches('\u{feff}');
            (i + 1, l.chars().count() + 1)
        }
        None => (line, 1),
    }
}

/// Mensagem do `saphyr-parser` em português, numa linha só.
fn message_pt(info: &str) -> String {
    let pt = if info.contains("mapping values are not allowed") {
        "valor de mapa fora de lugar (verifique a indentação)"
    } else if info.contains("did not find expected key") {
        "esperava uma chave nesta linha (verifique a indentação)"
    } else if info.contains("cannot start any token") || info.contains("tabs") {
        "caractere inválido aqui (tabulação não vale como indentação)"
    } else if info.contains("expected ',' or ']'") {
        "falta `,` ou `]`"
    } else if info.contains("expected ',' or '}'") {
        "falta `,` ou `}`"
    } else if info.contains("block collection") || info.contains("block mapping") {
        "bloco mal indentado"
    } else if info.contains("expected node content") {
        "esperava um valor aqui"
    } else if info.contains("unexpected end of stream") {
        "fim inesperado do arquivo (falta fechar aspas, colchete ou chave)"
    } else if info.contains("unknown anchor") {
        "alias (`*`) para uma âncora que não existe"
    } else {
        return format!("sintaxe inválida: {}", info.split_whitespace().collect::<Vec<_>>().join(" "));
    };
    pt.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ok(text: &str) -> Value {
        match parse(text, &Limits::default()) {
            Ok(v) => v,
            Err(e) => panic!("{text:?} deveria ler: {e:?}"),
        }
    }

    fn erro(text: &str) -> SyntaxError {
        match parse(text, &Limits::default()) {
            Err(YamlError::Syntax(e)) => e,
            outro => panic!("{text:?} deveria dar erro de sintaxe: {outro:?}"),
        }
    }

    #[test]
    fn mapa_simples_vira_objeto_com_tipos() {
        let v = ok("a: 1\nb: true\nc: ~\nd: texto\ne: 1.5\nf: \"1\"\ng: yes\n");
        assert_eq!(v, json!({"a": 1, "b": true, "c": null, "d": "texto", "e": 1.5, "f": "1", "g": "yes"}));
    }

    #[test]
    fn escalares_do_esquema_core() {
        let v = ok("- 0x1F\n- 0o17\n- -3\n- +4\n- 1e3\n- .inf\n- .nan\n- NULL\n- \n- TRUE\n- False\n- 1.2.3\n- '~'\n");
        assert_eq!(v, json!([31, 15, -3, 4, 1000.0, ".inf", ".nan", null, null, true, false, "1.2.3", "~"]));
    }

    #[test]
    fn lista_de_mapas() {
        let v = ok("- nome: a\n  n: 1\n- nome: b\n  n: 2\n");
        assert_eq!(v, json!([{"nome": "a", "n": 1}, {"nome": "b", "n": 2}]));
    }

    #[test]
    fn bloco_literal_e_dobrado_viram_texto() {
        let v = ok("a: |\n  12\n  linha 2\nb: >\n  dobrado\n  junto\n");
        assert_eq!(v, json!({"a": "12\nlinha 2\n", "b": "dobrado junto\n"}));
    }

    #[test]
    fn ancora_e_alias_clonam_o_valor() {
        let v = ok("base: &b {x: 1}\ncopia: *b\n");
        assert_eq!(v["copia"], json!({"x": 1}));
        assert_eq!(v["base"], json!({"x": 1}));
    }

    #[test]
    fn merge_key_mescla_e_o_proprio_mapa_vence() {
        let v = ok("b: &b {x: 1, y: 2}\nc:\n  <<: *b\n  y: 9\n");
        assert_eq!(v["c"], json!({"x": 1, "y": 9}));
        // A chave explícita vence mesmo vindo antes do merge; numa lista, o primeiro mapa vence.
        let v = ok("p: &p {x: 1}\nq: &q {x: 2, z: 3}\nr:\n  x: 0\n  <<: [*p, *q]\n");
        assert_eq!(v["r"], json!({"x": 0, "z": 3}));
        // `<<` entre aspas é uma chave comum.
        let v = ok("b: &b {x: 1}\nc: {\"<<\": *b}\n");
        assert_eq!(v["c"], json!({"<<": {"x": 1}}));
    }

    #[test]
    fn varios_documentos_viram_lista() {
        let v = ok("a: 1\n---\nb: 2\n");
        assert_eq!(v, json!([{"a": 1}, {"b": 2}]));
    }

    #[test]
    fn um_documento_nao_vira_lista() {
        assert_eq!(ok("---\na: 1\n"), json!({"a": 1}));
        assert_eq!(ok("- 1\n"), json!([1]));
    }

    #[test]
    fn vazio_e_so_comentario_sao_empty() {
        for t in ["", "# so comentario\n", "\u{feff}\n  \n"] {
            assert!(matches!(parse(t, &Limits::default()), Err(YamlError::Empty)), "{t:?}");
        }
    }

    #[test]
    fn tags_nao_sao_erro() {
        let v = ok("v: !Ref Foo\nw: !Sub \"${A}\"\nn: !!str 12\n");
        assert_eq!(v, json!({"v": "Foo", "w": "${A}", "n": "12"}));
    }

    #[test]
    fn chave_duplicada_nao_e_erro() {
        assert_eq!(ok("a: 1\na: 2\n")["a"], json!(2));
    }

    #[test]
    fn chaves_nao_texto_e_compostas() {
        let v = ok("1: a\ntrue: b\n? [x, y]\n: c\n");
        assert_eq!(v, json!({"1": "a", "true": "b", "[\"x\",\"y\"]": "c"}));
    }

    #[test]
    fn chave_composta_longa_e_cortada() {
        let longa = format!("? [{}]\n: v\n", "x, ".repeat(300));
        let v = ok(&longa);
        let chave = v.as_object().unwrap().keys().next().unwrap().clone();
        assert_eq!(chave.chars().count(), MAX_KEY_CHARS);
    }

    #[test]
    fn erros_de_sintaxe_apontam_a_linha() {
        for (texto, linha) in [("a:\n  b: 1\n c: 2\n", 3), ("a: b: c\n", 1), ("a:\n\tb: 1\n", 2)] {
            let e = erro(texto);
            assert_eq!(e.line, linha, "{texto:?}: {e:?}");
            assert!(!e.message.is_empty());
            assert!(!e.message.contains("expected") && !e.message.contains("did not find"), "{}", e.message);
            assert!(!e.message.contains('\n') && !e.message.contains('\t'));
        }
        assert_eq!(erro("a: b: c\n").col, 5);
    }

    #[test]
    fn fim_do_arquivo_sem_quebra_final() {
        let e = erro("a: [1, 2");
        assert_eq!((e.line, e.col), (1, 9));
        assert_eq!(erro("a: {b: 1\n\n\n").line, 1);
    }

    #[test]
    fn yaml_valido_nunca_e_erro_de_sintaxe() {
        ok("a: &a 1\nb: *a\n---\nc: |\n  x\n...\n");
        ok("? {a: 1}\n: mapa como chave\n");
        ok("tpl: &t\n  x: 1\nuse:\n  <<: *t\n");
        ok("auto: &a [*a]\n");
    }

    #[test]
    fn bomba_de_aliases_estoura_o_limite_pequeno() {
        let limits = Limits { max_nodes: 1000, max_depth: 128, max_string_bytes: 1 << 20 };
        let mut t = String::from("l0: &l0 [x, x]\n");
        for i in 1..=12 {
            t.push_str(&format!("l{i}: &l{i} [*l{p}, *l{p}]\n", p = i - 1));
        }
        assert!(matches!(parse(&t, &limits), Err(YamlError::Limit(m)) if m == TOO_BIG));
    }

    #[test]
    fn alias_auto_referente_conta_no_teto() {
        let limits = Limits { max_nodes: 10, ..Limits::default() };
        // Sem fechar o `]`: o teto tem de cair durante a leitura, não só quando a âncora fecha.
        let t = format!("x: &a [{}", "*a, ".repeat(20));
        assert!(matches!(parse(&t, &limits), Err(YamlError::Limit(m)) if m == TOO_BIG));
    }

    #[test]
    fn aninhamento_fundo_para_no_teto() {
        let limits = Limits { max_depth: 64, ..Limits::default() };
        // 500 colchetes passam do teto do próprio parser (255); 100 e o bloco param no nosso.
        for t in ["[".repeat(500), "[".repeat(100), "- ".repeat(100), "{a: ".repeat(100)] {
            assert!(matches!(parse(&t, &limits), Err(YamlError::Limit(m)) if m == TOO_DEEP), "{}", &t[..8]);
        }
        assert!(parse(&"[".repeat(64), &limits).is_err_and(|e| matches!(e, YamlError::Syntax(_))));
    }

    /// Exemplos válidos de `samples/`: nome e texto, todos aceitos com os tetos padrão.
    const EXEMPLOS: &[(&str, &str)] = &[
        ("config.yaml", include_str!("../samples/config.yaml")),
        ("k8s.yaml", include_str!("../samples/k8s.yaml")),
        ("github-actions.yml", include_str!("../samples/github-actions.yml")),
        ("cloudformation.yaml", include_str!("../samples/cloudformation.yaml")),
        ("ancoras.yaml", include_str!("../samples/ancoras.yaml")),
    ];

    #[test]
    fn exemplos_reais_sao_aceitos() {
        for (nome, texto) in EXEMPLOS {
            if let Err(e) = parse(texto, &Limits::default()) {
                panic!("{nome} deveria ser aceito: {e:?}");
            }
        }
    }

    #[test]
    fn exemplos_com_armadilhas_nao_dao_falso_erro() {
        let config = ok(include_str!("../samples/config.yaml"));
        // Tabulação dentro de string (não na indentação) e chave repetida (a última vence).
        assert!(config.to_string().contains("\\t"), "{config}");
        let k8s = ok(include_str!("../samples/k8s.yaml"));
        assert_eq!(k8s.as_array().map(Vec::len), Some(2), "Deployment + Service");
        let cf = ok(include_str!("../samples/cloudformation.yaml"));
        let texto = cf.to_string();
        assert!(texto.contains("AppBucket") && texto.contains("${AWS::StackName}"), "{texto}");
    }

    #[test]
    fn ancoras_do_sample_resolvem_merge() {
        let v = ok(include_str!("../samples/ancoras.yaml"));
        let s = &v["servicos"];
        assert_eq!(s["web"]["imagem"], json!("nginx:1.27"));
        assert_eq!(s["web"]["replicas"], json!(2));
        assert_eq!(s["api"]["replicas"], json!(4), "a chave explícita vence o merge");
        assert_eq!(s["api"]["recursos"], json!({"cpu": "500m", "memoria": "256Mi"}));
        assert_eq!(v["expostas"], v["portas"]);
    }

    #[test]
    fn exemplo_quebrado_aponta_a_linha_conhecida() {
        let e = erro(include_str!("../samples/quebrado.yaml"));
        assert_eq!(e.line, 5, "{e:?}");
    }

    #[test]
    fn crlf_e_bom() {
        assert_eq!(erro("\u{feff}a: 1\r\nb: [\r\n").line, 2);
        assert_eq!(ok("\u{feff}a: 1\r\nb: 2\r\n"), json!({"a": 1, "b": 2}));
    }
}
