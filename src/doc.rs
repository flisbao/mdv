//! O documento aberto: texto, opções de leitura e como o texto vira linhas.

use crate::{cli, data, image, render, yaml_view};

/// Como a entrada é interpretada: o formato (com as opções que só ele aceita) e a rolagem de código.
#[derive(Clone)]
pub(crate) struct DocOpts {
    pub(crate) doc: cli::Doc,
    pub(crate) code_scroll: Option<usize>,
}

impl DocOpts {
    pub(crate) fn from_cli(cli: &cli::Cli) -> Self {
        let doc = match &cli.command {
            cli::Command::View(d) => d.clone(),
            _ => cli::Doc::Markdown,
        };
        DocOpts { doc, code_scroll: None }
    }
}

/// Único ponto que decide como o texto da entrada vira linhas, conforme o modo (TUI e `--plain`).
pub(crate) fn render_doc(doc: &DocOpts, text: &str, width: usize, images: &image::ImageStore) -> render::Rendered {
    match &doc.doc {
        cli::Doc::Markdown => {
            render::render_with(
                text,
                &render::RenderOpts { width, images: Some(images), code_scroll: doc.code_scroll },
            )
        }
        cli::Doc::Json { json_path, view } => {
            let opts = data::JsonOpts {
                path: json_path.clone(),
                view: *view,
                head: cli::DEFAULT_HEAD,
                cols: Vec::new(),
            };
            data::render_json(text, &opts, width)
        }
        cli::Doc::Yaml { yaml_path, view } => {
            let opts = data::JsonOpts {
                path: yaml_path.clone(),
                view: *view,
                head: cli::DEFAULT_HEAD,
                cols: Vec::new(),
            };
            yaml_view::render_yaml(text, &opts, width)
        }
        cli::Doc::Table { delimiter, head, cols, no_header } => {
            let opts = data::TableOpts {
                delimiter: *delimiter,
                head: *head,
                cols: cols.clone(),
                no_header: *no_header,
            };
            data::render_table_data(text, &opts, width)
        }
    }
}

/// Linguagem usada para mostrar a fonte crua (`m`).
pub(crate) fn raw_lang(doc: &cli::Doc) -> &'static str {
    match doc {
        cli::Doc::Markdown => "md",
        cli::Doc::Json { .. } => "json",
        cli::Doc::Yaml { .. } => "yaml",
        cli::Doc::Table { .. } => "text",
    }
}

/// O conteúdo aberto: o texto, como lê-lo e o resultado já renderizado.
pub(crate) struct Document {
    pub(crate) md: String,
    pub(crate) rendered: render::Rendered,
    pub(crate) render_width: usize,
    pub(crate) dirty: bool,
    pub(crate) opts: DocOpts,
}

impl Document {
    pub(crate) fn new(md: String, opts: DocOpts) -> Self {
        Document { md, rendered: render::Rendered::default(), render_width: 0, dirty: true, opts }
    }

    /// Troca o texto (tecla `r`, abrir outro arquivo).
    pub(crate) fn set_md(&mut self, new: String) {
        self.md = new;
        self.dirty = true;
    }

    /// Acrescenta um pedaço de stdin (`--follow`).
    pub(crate) fn append_md(&mut self, chunk: &str) {
        self.md.push_str(chunk);
        self.dirty = true;
    }

    /// Renderiza em `width`, sempre. `raw` mostra a fonte em vez do formato interpretado.
    pub(crate) fn render(&mut self, width: usize, raw: bool, images: &image::ImageStore) {
        self.rendered = if raw {
            render::render_code_only_at(&self.md, raw_lang(&self.opts.doc), width, self.opts.code_scroll)
        } else {
            images.begin_pass();
            render_doc(&self.opts, &self.md, width, images)
        };
        self.render_width = width;
        self.dirty = false;
    }

    /// O texto mudou ou a largura é outra?
    pub(crate) fn needs_render(&self, width: usize) -> bool {
        self.dirty || width != self.render_width
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts() -> DocOpts {
        DocOpts::from_cli(&crate::cli::parse(["x.md".to_string()]).expect("cli de teste"))
    }

    #[test]
    fn raw_lang_do_yaml() {
        assert_eq!(raw_lang(&cli::doc_for_path("a.yaml")), "yaml");
    }

    #[test]
    fn render_doc_yaml_colore() {
        let store = image::ImageStore::new(None);
        let doc = DocOpts { doc: cli::doc_for_path("a.yaml"), code_scroll: None };
        let r = render_doc(&doc, "nome: mdv\nlista:\n  - 1\n", 60, &store);
        let texto: String = r.lines.iter().flat_map(|l| l.spans.iter().map(|s| s.content.as_ref())).collect();
        assert!(texto.contains("nome"), "{texto}");
        let colorido = r.lines.iter().flat_map(|l| &l.spans).any(|s| s.style.fg.is_some());
        assert!(colorido, "o YAML deveria sair com cores");
    }

    #[test]
    fn novo_documento_nasce_sujo_e_sem_linhas() {
        let d = Document::new("# a\n".into(), opts());
        assert!(d.dirty);
        assert!(d.rendered.lines.is_empty());
    }

    #[test]
    fn render_limpa_dirty_e_needs_render_acompanha_a_largura() {
        let store = image::ImageStore::new(None);
        let mut d = Document::new("# a\n\ntexto\n".into(), opts());
        assert!(d.needs_render(60));
        d.render(60, false, &store);
        assert!(!d.dirty && !d.rendered.lines.is_empty());
        assert!(!d.needs_render(60), "mesma largura e limpo: nada a fazer");
        assert!(d.needs_render(40), "largura nova refaz");
    }

    #[test]
    fn set_md_e_append_md_pedem_novo_render() {
        let store = image::ImageStore::new(None);
        let mut d = Document::new("a\n".into(), opts());
        d.render(60, false, &store);
        d.append_md("b\n");
        assert_eq!(d.md, "a\nb\n");
        assert!(d.dirty);
        d.render(60, false, &store);
        d.set_md("c\n".into());
        assert_eq!(d.md, "c\n");
        assert!(d.dirty);
    }
}
