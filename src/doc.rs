//! O documento aberto: texto, opções de leitura e como o texto vira linhas.

use crate::{cli, data, image, render};

/// Como a entrada é interpretada: o modo e as opções que só valem para JSON ou tabela.
#[derive(Clone)]
pub(crate) struct DocOpts {
    pub(crate) mode: cli::Mode,
    pub(crate) json_path: Option<String>,
    pub(crate) json_view: cli::JsonView,
    pub(crate) head: usize,
    pub(crate) cols: Vec<String>,
    pub(crate) no_header: bool,
    pub(crate) code_scroll: Option<usize>,
}

impl DocOpts {
    pub(crate) fn from_cli(cli: &cli::Cli) -> Self {
        DocOpts {
            mode: cli.mode,
            json_path: cli.json_path.clone(),
            json_view: cli.json_view,
            head: cli.head,
            cols: cli.cols.clone(),
            no_header: cli.no_header,
            code_scroll: None,
        }
    }
}

/// Único ponto que decide como o texto da entrada vira linhas, conforme o modo (TUI e `--plain`).
pub(crate) fn render_doc(doc: &DocOpts, text: &str, width: usize, images: &image::ImageStore) -> render::Rendered {
    match doc.mode {
        cli::Mode::Markdown => {
            render::render_with(
                text,
                &render::RenderOpts { width, images: Some(images), code_scroll: doc.code_scroll },
            )
        }
        cli::Mode::Json => {
            let opts = data::JsonOpts {
                path: doc.json_path.clone(),
                view: doc.json_view,
                head: doc.head,
                cols: doc.cols.clone(),
            };
            data::render_json(text, &opts, width)
        }
        cli::Mode::Table { delimiter } => {
            let opts = data::TableOpts {
                delimiter,
                head: doc.head,
                cols: doc.cols.clone(),
                no_header: doc.no_header,
            };
            data::render_table_data(text, &opts, width)
        }
    }
}

/// Linguagem usada para mostrar a fonte crua (`m`).
pub(crate) fn raw_lang(mode: cli::Mode) -> &'static str {
    match mode {
        cli::Mode::Markdown => "md",
        cli::Mode::Json => "json",
        cli::Mode::Table { .. } => "text",
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
            render::render_code_only_at(&self.md, raw_lang(self.opts.mode), width, self.opts.code_scroll)
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
        DocOpts::from_cli(&match crate::cli::parse(["x.md".to_string()]) {
            Ok(crate::cli::Parsed::Run(c)) => c,
            _ => panic!("cli de teste"),
        })
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
