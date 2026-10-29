//! Vistas do YAML (texto, tabela, esquema) sobre o `Value` lido por `yamldoc`, reaproveitando as
//! vistas de JSON de `data`. O texto mostra o YAML original (comentários e ordem preservados).

use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use crate::cli::JsonView;
use crate::data::{self, JsonOpts};
use crate::mem;
use crate::render::{error_rendered, memory_error, render_code_only, Rendered, DIM};
use crate::syntax_error::render_with_error;
use crate::yamldoc::{self, Limits, YamlError};

pub fn render_yaml(text: &str, o: &JsonOpts, width: usize) -> Rendered {
    render_yaml_with(text, o, width, &Limits::default())
}

/// A vista que `Auto` escolheria para este texto (usada pela tecla `v`). `Text` se não parsear.
pub fn auto_view_of_text(text: &str, path: Option<&str>) -> JsonView {
    let Ok(root) = yamldoc::parse(text, &Limits::default()) else {
        return JsonView::Text;
    };
    match data::select(&root, path.unwrap_or("")) {
        Ok(sel) => data::auto_view(sel),
        Err(_) => JsonView::Text,
    }
}

/// Mesma ordem de checagens de `data::render_json`; `limits` entra por parâmetro para os testes.
fn render_yaml_with(text: &str, o: &JsonOpts, width: usize, limits: &Limits) -> Rendered {
    if text.len() > data::MAX_DATA_BYTES {
        return error_rendered("entrada grande demais para o modo YAML (limite de 5 MiB)");
    }
    if let Err(e) = mem::check_json(text.len()) {
        return error_rendered(&e.to_string());
    }
    let text = text.trim_start_matches('\u{feff}');
    let text_view = matches!(o.view, JsonView::Text | JsonView::Auto);
    let root = match yamldoc::parse(text, limits) {
        Ok(v) => v,
        Err(YamlError::Syntax(e)) => return render_with_error(text, "yaml", width, &e),
        Err(YamlError::Empty) => return error_rendered("arquivo vazio"),
        // Sem o `Value` não há tabela nem esquema; o texto continua legível.
        Err(YamlError::Limit(_)) if text_view => {
            return memory_error().unwrap_or_else(|| render_code_only(text, "yaml", width));
        }
        Err(YamlError::Limit(m)) => {
            let hint = "use a vista de texto";
            let msg = if m.contains(hint) { m } else { format!("{m} ({hint}: tecla v ou --as text)") };
            return error_rendered(&msg);
        }
    };
    if let Some(err) = memory_error() {
        return err;
    }
    let sel = match data::select(&root, o.path.as_deref().unwrap_or("")) {
        Ok(v) => v,
        Err(e) => return error_rendered(&e),
    };
    let view = if o.view == JsonView::Auto { data::auto_view(sel) } else { o.view };
    let out = match view {
        JsonView::Table => data::render_json_table(sel, o, width),
        JsonView::Schema => data::render_json_schema(sel, width),
        JsonView::Text | JsonView::Auto if has_path(o) => data::render_json_text(sel, width),
        JsonView::Text | JsonView::Auto => text_with_summary(text, &root, width),
    };
    memory_error().unwrap_or(out)
}

/// `--path` que de fato recorta o documento (vazio e `.` valem a raiz).
fn has_path(o: &JsonOpts) -> bool {
    !matches!(o.path.as_deref().map(str::trim), None | Some("") | Some("."))
}

/// O YAML original colorido, com o resumo do documento em itálico cinza na primeira linha.
fn text_with_summary(text: &str, root: &serde_json::Value, width: usize) -> Rendered {
    let mut r = render_code_only(text, "yaml", width);
    let head = Line::from(Span::styled(
        data::describe(root),
        Style::default().fg(DIM).add_modifier(Modifier::ITALIC),
    ));
    r.lines.insert(0, head);
    r
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::Color;

    fn opts(path: Option<&str>, view: JsonView) -> JsonOpts {
        JsonOpts { path: path.map(String::from), view, head: 100, cols: Vec::new() }
    }

    fn line_text(l: &Line) -> String {
        l.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    fn text(r: &Rendered) -> String {
        r.lines.iter().map(line_text).collect::<Vec<_>>().join("\n")
    }

    /// Cores dos trechos de conteúdo de uma linha (sem a moldura `│ ` nem o preenchimento).
    fn fgs(l: &Line) -> Vec<Color> {
        l.spans.iter().skip(1).filter(|s| !s.content.trim().is_empty()).filter_map(|s| s.style.fg).collect()
    }

    fn tiny() -> Limits {
        Limits { max_nodes: 2, ..Limits::default() }
    }

    #[test]
    fn texto_colore_por_tipo() {
        let r = render_yaml("nome: mdv\nversao: 3\nativo: true\nvazio: null\n# nota\n", &opts(None, JsonView::Text), 80);
        let mut cores: Vec<Color> = r.lines.iter().flat_map(fgs).collect();
        cores.sort_by_key(|c| format!("{c:?}"));
        cores.dedup();
        assert!(cores.len() >= 4, "{cores:?}\n{}", text(&r));
        let nota = r.lines.iter().find(|l| line_text(l).contains("# nota")).expect("linha da nota");
        let nome = r.lines.iter().find(|l| line_text(l).contains("nome: mdv")).expect("linha do nome");
        assert_ne!(fgs(nota).first(), fgs(nome).first());
    }

    #[test]
    fn texto_preserva_comentarios_e_ordem() {
        let r = render_yaml("nome: mdv\nversao: 3\n# nota\n", &opts(None, JsonView::Text), 80);
        let t = text(&r);
        assert!(t.contains("# nota"), "{t}");
        assert!(t.find("nome").unwrap() < t.find("versao").unwrap(), "{t}");
        assert!(t.contains("objeto · 2 chaves"), "{t}");
    }

    #[test]
    fn lista_de_objetos_abre_em_tabela() {
        let src = "- {id: 1, nome: a}\n- {id: 2, nome: b}\n";
        let r = render_yaml(src, &opts(None, JsonView::Auto), 80);
        let t = text(&r);
        assert!(t.contains("nome"), "{t}");
        assert!(t.contains("│ 1") || t.contains("1 │"), "{t}");
        assert_eq!(auto_view_of_text(src, None), JsonView::Table);
        assert_eq!(auto_view_of_text("a: 1\n", None), JsonView::Text);
        assert_eq!(auto_view_of_text("a: [", None), JsonView::Text);
    }

    #[test]
    fn esquema_lista_chaves_e_tipos() {
        let t = text(&render_yaml("a: 1\nb: [x]\n", &opts(None, JsonView::Schema), 80));
        assert!(t.contains("número") && t.contains("lista"), "{t}");
    }

    #[test]
    fn path_seleciona_trecho_em_json() {
        let t = text(&render_yaml("a:\n  b: {c: 1}\n", &opts(Some(".a.b"), JsonView::Text), 80));
        assert!(t.contains("\"c\": 1"), "{t}");
    }

    #[test]
    fn path_inexistente_vira_erro() {
        let t = text(&render_yaml("a: 1\n", &opts(Some(".x"), JsonView::Auto), 80));
        assert!(t.contains("'x'"), "{t}");
    }

    #[test]
    fn erro_de_sintaxe_mostra_texto_e_mensagem() {
        let t = text(&render_yaml("a:\n  b: 1\n c: 2\n", &opts(None, JsonView::Text), 80));
        assert!(t.contains("c: 2") && t.contains("✗ linha 3"), "{t}");
    }

    #[test]
    fn erro_aparece_em_qualquer_vista() {
        for v in [JsonView::Auto, JsonView::Table, JsonView::Schema] {
            let t = text(&render_yaml("a:\n  b: 1\n c: 2\n", &opts(None, v), 80));
            assert!(t.contains("✗ linha 3"), "{v:?}\n{t}");
        }
    }

    #[test]
    fn bom_nao_desloca_a_linha_do_erro() {
        let t = text(&render_yaml("\u{feff}a:\n  b: 1\n c: 2\n", &opts(None, JsonView::Text), 80));
        assert!(t.contains("✗ linha 3") && !t.contains('\u{feff}'), "{t}");
    }

    #[test]
    fn limite_estourado_mantem_a_vista_de_texto() {
        let src = "a: 1\nb: 2\nc: 3\n";
        for v in [JsonView::Text, JsonView::Auto] {
            let t = text(&render_yaml_with(src, &opts(None, v), 80, &tiny()));
            assert!(t.contains("b: 2") && !t.contains("✗"), "{v:?}\n{t}");
        }
        for v in [JsonView::Table, JsonView::Schema] {
            let t = text(&render_yaml_with(src, &opts(None, v), 80, &tiny()));
            assert_eq!(t.matches("use a vista de texto").count(), 1, "{v:?}\n{t}");
        }
    }

    #[test]
    fn arquivo_vazio() {
        let r = render_yaml("", &opts(None, JsonView::Auto), 80);
        assert_eq!(r.lines.len(), 1);
        assert!(text(&r).contains("arquivo vazio"));
    }

    #[test]
    fn entrada_grande_demais_e_recusada() {
        let big = "a".repeat(data::MAX_DATA_BYTES + 1);
        assert!(text(&render_yaml(&big, &opts(None, JsonView::Text), 80)).contains("limite de 5 MiB"));
    }
}
