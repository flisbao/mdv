//! Vistas do XML (árvore, tabela, esquema) sobre o `Value` lido por `xmldoc`, reaproveitando as
//! vistas de JSON de `data`. A árvore mostra o conteúdo (sem tags); a tecla `m` mostra o original.

use serde_json::Value;

use crate::cli::JsonView;
use crate::data::{self, JsonOpts};
use crate::mem;
use crate::render::{error_rendered, memory_error, render_code_only, Rendered};
use crate::syntax_error::render_with_error;
use crate::xmldoc::{self, Limits, XmlDoc, XmlError};

/// Teto da árvore já recuada: o recuo multiplica uma entrada pequena e muito aninhada.
const MAX_OUTLINE_BYTES: usize = 8 * 1024 * 1024;
/// Nós visitados à procura da lista de registros.
const MAX_RECORD_NODES: usize = 10_000;

pub fn render_xml(text: &str, o: &JsonOpts, width: usize) -> Rendered {
    render_xml_with(text, o, width, &Limits::default())
}

/// A vista que `Auto` escolheria para este texto (usada pela tecla `v`). `Text` se não parsear.
pub fn auto_view_of_text(text: &str, path: Option<&str>) -> JsonView {
    if text.len() > data::MAX_DATA_BYTES {
        return JsonView::Text;
    }
    let Ok(doc) = xmldoc::parse(text, &Limits::default()) else {
        return JsonView::Text;
    };
    if path_set(path) {
        return match data::select(&doc.value, path.unwrap_or("")) {
            Ok(sel) => data::auto_view(sel),
            Err(_) => JsonView::Text,
        };
    }
    if find_dominant_records(&doc.value).is_some() {
        JsonView::Table
    } else {
        JsonView::Text
    }
}

/// Mesma ordem de checagens de `yaml_view::render_yaml_with`; `limits` entra por parâmetro para os testes.
fn render_xml_with(text: &str, o: &JsonOpts, width: usize, limits: &Limits) -> Rendered {
    if text.len() > data::MAX_DATA_BYTES {
        // O texto já está decodificado (Latin-1 e UTF-16 podem crescer até 3x), então o arquivo pode
        // ter menos de 5 MiB: a mensagem diz o tamanho do texto.
        let mib = text.len().div_ceil(1024 * 1024);
        return error_rendered(&format!(
            "entrada grande demais para o modo XML (texto de {mib} MiB depois de decodificar; limite de 5 MiB)"
        ));
    }
    if let Err(e) = mem::check_xml(text.len()) {
        return error_rendered(&e.to_string());
    }
    let text = text.trim_start_matches('\u{feff}');
    let text_view = matches!(o.view, JsonView::Text | JsonView::Auto);
    let has_path = path_set(o.path.as_deref());
    let doc = match xmldoc::parse(text, limits) {
        Ok(d) => d,
        Err(XmlError::Syntax(e)) => return render_with_error(text, "xml", width, &e),
        Err(XmlError::Empty) => return error_rendered("arquivo vazio"),
        // Sem o `Value` não há tabela, esquema nem `--path`; o XML continua legível, com aviso.
        Err(XmlError::Limit(m)) if text_view && !has_path => {
            return memory_error().unwrap_or_else(|| xml_with_notice(text, &m, width));
        }
        // As mensagens de limite já dizem para usar a vista de texto.
        Err(XmlError::Limit(m)) => return memory_error().unwrap_or_else(|| error_rendered(&m)),
    };
    if let Some(err) = memory_error() {
        return err;
    }
    let out = render_doc(&doc, o, has_path, width);
    memory_error().unwrap_or(out)
}

fn render_doc(doc: &XmlDoc, o: &JsonOpts, has_path: bool, width: usize) -> Rendered {
    // Sem `--path`, tabela e automático preferem a lista de registros; com ele, o trecho escolhido.
    let (sel, records) = if has_path {
        match data::select(&doc.value, o.path.as_deref().unwrap_or("")) {
            Ok(v) => (v, None),
            Err(e) => return error_rendered(&e),
        }
    } else {
        // `Auto` só abre a tabela se a lista domina o documento; `Table` aceita a mais rasa.
        let found = match o.view {
            JsonView::Auto => find_dominant_records(&doc.value),
            JsonView::Table => find_records(&doc.value),
            JsonView::Schema | JsonView::Text => None,
        };
        match found {
            Some((label, v)) => (v, Some(label)),
            None => (&doc.value, None),
        }
    };
    let view = if o.view == JsonView::Auto { data::auto_view(sel) } else { o.view };
    match view {
        JsonView::Table => {
            let r = data::render_json_table(sel, o, width);
            match (records, sel) {
                (Some(label), Value::Array(items)) => {
                    r.with_head(format!("registros em {label} · {} itens", items.len()))
                }
                _ => r,
            }
        }
        JsonView::Schema => data::render_json_schema(sel, width),
        JsonView::Text | JsonView::Auto => {
            let head = if has_path { data::describe(sel) } else { format!("{} · {}", doc.root, data::describe(sel)) };
            tree_view(sel, head, width)
        }
    }
}

/// `--path` informado: só vazio (depois do `trim`) vale "sem caminho"; qualquer outro, inclusive
/// `.`, passa por `data::select` (que devolve a raiz para `.`) e dispensa a busca de registros.
fn path_set(path: Option<&str>) -> bool {
    !matches!(path.map(str::trim), None | Some(""))
}

/// Teto estourado: o XML original com o motivo (`m` até o `:`) no lugar do resumo, para explicar
/// por que a tecla `v` leva a um erro.
fn xml_with_notice(text: &str, m: &str, width: usize) -> Rendered {
    let motivo = m.split(':').next().unwrap_or(m);
    render_code_only(text, "xml", width).with_head(format!("{motivo} para tabela/esquema: só o XML original"))
}

fn tree_view(v: &Value, head: String, width: usize) -> Rendered {
    let Some(s) = outline(v) else {
        return error_rendered("saída grande demais depois de indentada (limite de 8 MiB)");
    };
    render_code_only(&s, "yaml", width).with_head(head)
}

/// O `Array` de objetos mais raso (largura primeiro, sem descer em listas), o primeiro na ordem do
/// documento em caso de empate, com o caminho de chaves separado por ` › `.
fn find_records(root: &Value) -> Option<(String, &Value)> {
    let (path, v) = records_path(root)?;
    Some((path.join(" › "), v))
}

/// `find_records`, mas só se a lista *domina* o documento: nenhum objeto no caminho da raiz até ela
/// tem outro filho que seja objeto ou lista (texto e `@atributos` não contam). Numa NF-e, por
/// exemplo, as duplicatas (`cobr › dup`) ficam ao lado de emitente, destinatário e totais: a
/// tabela esconderia o essencial, então o `Auto` mostra a árvore.
fn find_dominant_records(root: &Value) -> Option<(String, &Value)> {
    let (path, v) = records_path(root)?;
    let mut node = root;
    for key in &path {
        let Value::Object(m) = node else { return None };
        if m.iter().any(|(k, c)| k != key && matches!(c, Value::Object(_) | Value::Array(_))) {
            return None;
        }
        node = m.get(*key)?;
    }
    Some((path.join(" › "), v))
}

/// Caminho de chaves até a lista de registros mais rasa, e a lista.
fn records_path(root: &Value) -> Option<(Vec<&str>, &Value)> {
    let mut queue = std::collections::VecDeque::new();
    queue.push_back((Vec::<&str>::new(), root));
    let mut visited = 0;
    while let Some((path, node)) = queue.pop_front() {
        let Value::Object(m) = node else { continue };
        for (k, v) in m {
            visited += 1;
            if visited > MAX_RECORD_NODES || mem::exceeded() {
                return None;
            }
            match v {
                Value::Array(a) if !a.is_empty() && a.iter().all(Value::is_object) => {
                    let mut p = path.clone();
                    p.push(k);
                    return Some((p, v));
                }
                Value::Object(_) => {
                    let mut p = path.clone();
                    p.push(k);
                    queue.push_back((p, v));
                }
                _ => {}
            }
        }
    }
    None
}

/// A árvore recuada (`chave: valor`); `None` se passar de `MAX_OUTLINE_BYTES` ou da memória.
fn outline(v: &Value) -> Option<String> {
    outline_capped(v, MAX_OUTLINE_BYTES)
}

/// `outline` com o teto de bytes por parâmetro (os testes usam um teto pequeno).
fn outline_capped(v: &Value, cap: usize) -> Option<String> {
    let mut out = String::new();
    node(&mut out, cap, 0, "", None, v)?;
    Some(out)
}

fn indent(out: &mut String, n: usize) {
    out.extend(std::iter::repeat_n(' ', n));
}

/// Escreve `v` com a linha de abertura em `ind` + `lead` (`- ` num item de lista) + `label`.
/// Os filhos ficam 2 colunas depois do começo do conteúdo (`ind + lead`).
fn node(out: &mut String, cap: usize, ind: usize, lead: &str, label: Option<&str>, v: &Value) -> Option<()> {
    if out.len() > cap || mem::exceeded() {
        return None;
    }
    let col = ind + lead.len();
    match (v, label) {
        (Value::Object(m), None) => {
            if m.is_empty() {
                indent(out, ind);
                out.push_str(lead.trim_end());
                out.push_str("{}\n");
            }
            // A primeira entrada leva o `- `; as demais alinham com ela.
            for (i, (k, c)) in m.iter().enumerate() {
                if i == 0 {
                    node(out, cap, ind, lead, Some(k), c)?;
                } else {
                    node(out, cap, col, "", Some(k), c)?;
                }
            }
        }
        (Value::Array(a), None) => {
            indent(out, ind);
            out.push_str(lead.trim_end());
            out.push('\n');
            for c in a {
                node(out, cap, col + 2, "- ", None, c)?;
            }
        }
        (Value::Object(m), Some(k)) => {
            head_line(out, ind, lead, k, "");
            for (k, c) in m {
                node(out, cap, col + 2, "", Some(k), c)?;
            }
        }
        (Value::Array(a), Some(k)) => {
            head_line(out, ind, lead, k, "");
            for c in a {
                node(out, cap, col + 2, "- ", None, c)?;
            }
        }
        (leaf, label) => {
            let s = match leaf {
                Value::String(s) => s.clone(),
                Value::Null => String::new(),
                other => other.to_string(),
            };
            indent(out, ind);
            out.push_str(lead);
            let multiline = s.contains('\n');
            match label {
                Some(k) => {
                    out.push_str(k);
                    out.push(':');
                    if multiline {
                        out.push_str(" |");
                    } else if !s.is_empty() {
                        out.push(' ');
                        out.push_str(&s);
                    }
                }
                None if multiline => out.push('|'),
                None => out.push_str(&s),
            }
            if label.is_none() && s.is_empty() {
                while out.ends_with(' ') {
                    out.pop();
                }
            }
            out.push('\n');
            if multiline {
                for l in s.split('\n') {
                    // Um texto só pode escrever muito (cada linha leva o recuo): confere por linha.
                    if out.len() > cap || mem::exceeded() {
                        return None;
                    }
                    if !l.is_empty() {
                        indent(out, col + 2);
                        out.push_str(l);
                    }
                    out.push('\n');
                }
            }
        }
    }
    Some(())
}

fn head_line(out: &mut String, ind: usize, lead: &str, key: &str, rest: &str) {
    indent(out, ind);
    out.push_str(lead);
    out.push_str(key);
    out.push(':');
    out.push_str(rest);
    out.push('\n');
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::Modifier;
    use ratatui::text::Line;
    use serde_json::json;

    use crate::render::DIM;

    fn opts(path: Option<&str>, view: JsonView) -> JsonOpts {
        JsonOpts { path: path.map(String::from), view, head: 100, cols: Vec::new() }
    }

    fn line_text(l: &Line) -> String {
        l.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    fn text(r: &Rendered) -> String {
        r.lines.iter().map(line_text).collect::<Vec<_>>().join("\n")
    }

    fn tiny() -> Limits {
        Limits { max_nodes: 2, ..Limits::default() }
    }

    #[test]
    fn outline_formata_objeto_lista_e_nulo() {
        let v = json!({"@id":"1","nome":"Ana","ender":{"rua":"X"},"tel":["1","2"],"v":null});
        assert_eq!(outline(&v).unwrap(), "@id: 1\nnome: Ana\nender:\n  rua: X\ntel:\n  - 1\n  - 2\nv:\n");
    }

    #[test]
    fn outline_item_objeto_alinha_as_entradas() {
        let v = json!({"det":[{"@n":"1","p":"a"},{"@n":"2","p":"b"}]});
        assert_eq!(outline(&v).unwrap(), "det:\n  - @n: 1\n    p: a\n  - @n: 2\n    p: b\n");
    }

    #[test]
    fn outline_texto_multilinha_e_raiz_escalar() {
        assert_eq!(outline(&json!({"t":"a\nb"})).unwrap(), "t: |\n  a\n  b\n");
        assert_eq!(outline(&json!("oi")).unwrap(), "oi\n");
    }

    #[test]
    fn outline_corta_texto_multilinha_no_teto() {
        let v = json!({"t": "a\n".repeat(200)});
        assert!(outline_capped(&v, 100).is_none());
        assert!(outline_capped(&v, 10_000).is_some());
        // O teto vale também para a folha raiz.
        assert!(outline_capped(&json!("a\n".repeat(200)), 100).is_none());
    }

    #[test]
    fn find_records_prefere_o_mais_raso_e_o_primeiro() {
        let v = json!({"cab":{"x":[{"a":"1"},{"a":"2"}]},"y":[{"b":"1"},{"b":"2"}]});
        assert_eq!(find_records(&v).unwrap().0, "y");
        let v = json!({"a":[{"k":"1"},{"k":"2"}],"b":[{"k":"3"},{"k":"4"}]});
        assert_eq!(find_records(&v).unwrap().0, "a");
        let v = json!({"i":["1",{"a":"2"}]});
        assert!(find_records(&v).is_none());
        let v = json!({"cab":{"x":[{"a":"1"}]}});
        assert_eq!(find_records(&v).unwrap().0, "cab › x");
    }

    #[test]
    fn lista_de_elementos_abre_em_tabela() {
        let src = "<lista><item id=\"1\"><nome>a</nome></item><item id=\"2\"><nome>b</nome></item></lista>";
        let t = text(&render_xml(src, &opts(None, JsonView::Auto), 80));
        assert!(t.contains("@id") && t.contains("nome") && t.contains('│'), "{t}");
        assert!(t.contains("registros em item"), "{t}");
        assert_eq!(auto_view_of_text(src, None), JsonView::Table);
    }

    /// NF-e assinada de verdade: poucas listas pequenas (`dup`, `Transform`) escondidas no meio.
    const NFE_ASSINADA: &str = include_str!("../samples/nfe-assinada.xml");

    #[test]
    fn auto_so_abre_tabela_quando_a_lista_domina_o_documento() {
        // Lista simples, catálogo e envelope SOAP com irmãos escalares dominam o documento.
        let lista = "<lista><item id=\"1\"/><item id=\"2\"/></lista>";
        let catalogo = "<catalogo><produto id=\"1\"><n>a</n></produto><produto id=\"2\"><n>b</n></produto></catalogo>";
        let soap = "<Envelope><Body><Resp><status>ok</status><item><a>1</a></item><item><a>2</a></item></Resp></Body></Envelope>";
        for (nome, src) in [("lista", lista), ("catalogo", catalogo), ("soap", soap)] {
            assert_eq!(auto_view_of_text(src, None), JsonView::Table, "{nome}");
            let t = text(&render_xml(src, &opts(None, JsonView::Auto), 80));
            assert!(t.contains("registros em"), "{nome}\n{t}");
        }
        // Uma lista ao lado de outro objeto (ou de uma lista de textos) não domina: abre a árvore.
        let irmao_objeto = "<r><cab><x>1</x></cab><item><a>1</a></item><item><a>2</a></item></r>";
        let irmao_lista = "<r><tel>1</tel><tel>2</tel><item><a>1</a></item><item><a>2</a></item></r>";
        // O irmão está no avô da lista, não no pai.
        let irmao_do_pai = "<r><cab><x>1</x></cab><corpo><item><a>1</a></item><item><a>2</a></item></corpo></r>";
        for (nome, src) in [("objeto", irmao_objeto), ("lista de textos", irmao_lista), ("avô", irmao_do_pai)] {
            assert_eq!(auto_view_of_text(src, None), JsonView::Text, "{nome}");
            let t = text(&render_xml(src, &opts(None, JsonView::Auto), 80));
            assert!(!t.contains("registros em") && t.contains("item:"), "{nome}\n{t}");
        }
    }

    #[test]
    fn nfe_assinada_abre_em_arvore_no_auto_e_em_tabela_no_table() {
        let auto = text(&render_xml(NFE_ASSINADA, &opts(None, JsonView::Auto), 80));
        assert!(auto.contains("xNome: Indústria ACME Ltda") && auto.contains("vNF: 450.00"), "{auto}");
        assert!(!auto.contains("registros em"), "{auto}");
        assert_eq!(auto_view_of_text(NFE_ASSINADA, None), JsonView::Text);
        // `--as table` e a tecla `v` continuam com a lista mais rasa (as duplicatas).
        let tabela = text(&render_xml(NFE_ASSINADA, &opts(None, JsonView::Table), 80));
        assert!(tabela.contains("registros em NFe › infNFe › cobr › dup · 2 itens"), "{tabela}");
    }

    #[test]
    fn nfe_com_varios_itens_tambem_abre_em_arvore() {
        let src = NFE_ASSINADA.replace(
            "</det>",
            "</det><det nItem=\"2\"><prod><cProd>2</cProd></prod></det>",
        );
        assert_eq!(auto_view_of_text(&src, None), JsonView::Text);
        let t = text(&render_xml(&src, &opts(None, JsonView::Auto), 80));
        assert!(t.contains("emit:") && t.contains("dest:"), "{t}");
    }

    #[test]
    fn tabela_automatica_com_resumo_desloca_o_indice_da_tabela() {
        let src = "<lista><item id=\"1\"><nome>a</nome></item><item id=\"2\"><nome>b</nome></item></lista>";
        let r = render_xml(src, &opts(None, JsonView::Auto), 80);
        assert!(line_text(&r.lines[0]).starts_with("registros em item"));
        let t = &r.tables[0];
        assert!(line_text(&r.lines[t.start]).starts_with('┌'), "{}", text(&r));
        assert!(line_text(&r.lines[t.end - 1]).starts_with('└'), "{}", text(&r));
    }

    const NF: &str = "<nf xmlns=\"urn:x\"><emit><xNome>ACME</xNome></emit></nf>";

    #[test]
    fn registro_unico_abre_em_arvore_sem_tags() {
        let t = text(&render_xml(NF, &opts(None, JsonView::Auto), 80));
        assert!(t.contains("emit:") && t.contains("  xNome: ACME"), "{t}");
        assert!(!t.contains('<') && !t.contains('>') && !t.contains("xmlns"), "{t}");
        assert_eq!(auto_view_of_text(NF, None), JsonView::Text);
    }

    #[test]
    fn path_ponto_vale_a_raiz_e_nao_busca_registros() {
        let src = "<lista><item id=\"1\"><nome>a</nome></item><item id=\"2\"><nome>b</nome></item></lista>";
        let t = text(&render_xml(src, &opts(Some("."), JsonView::Table), 80));
        assert!(!t.contains("registros em"), "{t}");
        assert!(t.contains("chave") && t.contains("valor"), "{t}");
        let t = text(&render_xml(src, &opts(None, JsonView::Table), 80));
        assert!(t.contains("registros em item"), "{t}");
        assert_eq!(auto_view_of_text(src, Some(".")), JsonView::Text);
        assert_eq!(auto_view_of_text(src, None), JsonView::Table);
    }

    #[test]
    fn registro_unico_em_tabela_vira_chave_valor() {
        let t = text(&render_xml(NF, &opts(None, JsonView::Table), 80));
        assert!(t.contains("emit.xNome") && t.contains("ACME"), "{t}");
    }

    #[test]
    fn esquema_lista_chaves_e_tipos() {
        let t = text(&render_xml("<a><b>1</b></a>", &opts(None, JsonView::Schema), 80));
        assert!(t.lines().any(|l| l.starts_with("│ objeto ")), "{t}");
        assert!(t.lines().any(|l| l.starts_with("│   b: texto")), "{t}");
    }

    #[test]
    fn path_seleciona_trecho_na_arvore() {
        let t = text(&render_xml("<r><a><b>1</b></a><c>2</c></r>", &opts(Some(".a"), JsonView::Text), 80));
        assert!(t.contains("b: 1") && !t.contains("c: 2"), "{t}");
    }

    #[test]
    fn path_inexistente_vira_erro() {
        let t = text(&render_xml("<r><a>1</a></r>", &opts(Some(".x"), JsonView::Auto), 80));
        assert!(t.contains("'x'"), "{t}");
    }

    #[test]
    fn erro_de_sintaxe_aparece_em_qualquer_vista() {
        for v in [JsonView::Auto, JsonView::Table, JsonView::Schema, JsonView::Text] {
            let t = text(&render_xml("<a>\n<b>\n</a>", &opts(None, v), 80));
            assert!(t.contains("✗ linha 3"), "{v:?}\n{t}");
        }
    }

    #[test]
    fn doctype_mostra_erro_na_linha() {
        let src = "<!DOCTYPE a [<!ENTITY e SYSTEM \"file:///etc/passwd\">]>\n<a>&e;</a>";
        let t = text(&render_xml(src, &opts(None, JsonView::Auto), 80));
        assert!(t.contains("✗ linha 1") && t.contains("DOCTYPE") && !t.contains("root:"), "{t}");
    }

    #[test]
    fn bom_nao_desloca_a_linha_do_erro() {
        let t = text(&render_xml("\u{feff}<a>\n<b>\n</a>", &opts(None, JsonView::Text), 80));
        assert!(t.contains("✗ linha 3") && !t.contains('\u{feff}'), "{t}");
    }

    #[test]
    fn limite_estourado_mostra_o_xml_original_com_aviso() {
        let src = "<a><b>1</b><c>2</c></a>";
        for v in [JsonView::Auto, JsonView::Text] {
            let r = render_xml_with(src, &opts(None, v), 80, &tiny());
            let aviso = &r.lines[0];
            assert!(line_text(aviso).contains("estrutura grande demais para tabela/esquema: só o XML original"), "{v:?}\n{}", text(&r));
            assert!(aviso.spans.iter().all(|s| s.style.fg == Some(DIM) && s.style.add_modifier.contains(Modifier::ITALIC)));
            assert!(text(&r).contains("<c>2</c>"), "{v:?}\n{}", text(&r));
        }
        for v in [JsonView::Table, JsonView::Schema] {
            let t = text(&render_xml_with(src, &opts(None, v), 80, &tiny()));
            assert_eq!(t.matches("use a vista de texto").count(), 1, "{v:?}\n{t}");
        }
    }

    #[test]
    fn aninhamento_fundo_mostra_o_xml_original() {
        let src = "<a>".repeat(100_000);
        let r = render_xml(&src, &opts(None, JsonView::Text), 80);
        assert!(line_text(&r.lines[0]).contains("aninhamento fundo demais"), "{}", line_text(&r.lines[0]));
    }

    #[test]
    fn arquivo_vazio() {
        let r = render_xml("", &opts(None, JsonView::Auto), 80);
        assert_eq!(r.lines.len(), 1);
        assert!(text(&r).contains("arquivo vazio"));
    }

    #[test]
    fn entrada_grande_demais_e_recusada() {
        let big = "a".repeat(data::MAX_DATA_BYTES + 1);
        let t = text(&render_xml(&big, &opts(None, JsonView::Text), 80));
        assert!(t.contains("limite de 5 MiB") && t.contains("texto de 6 MiB depois de decodificar"), "{t}");
        assert_eq!(auto_view_of_text(&big, None), JsonView::Text);
    }
}
