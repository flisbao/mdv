//! Estado de tela da TUI: `App` guarda o que aparece e o que o leitor está fazendo; o conteúdo vive em `Document`.

#[cfg(test)]
mod keys_tests;
mod search;
mod terminal;
#[cfg(test)]
mod tests;
mod ui;
mod viewport;
pub(crate) mod zoom;

use std::collections::HashSet;
use std::fmt::Write as _;
use std::io::{self, Write};
use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::Duration;

use ratatui::crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::{DefaultTerminal, Frame};
use unicode_width::UnicodeWidthStr;

use crate::doc::{DocOpts, Document};
use crate::load::{image_store, read_markdown_file, Source};
use crate::{browse, cli, clip, data, graphics, image, imgview, mem, picker, render, Failure};

pub(crate) use search::{highlight_line, SearchState};
pub(crate) use terminal::run_tui;
pub(crate) use viewport::Viewport;
pub(crate) use zoom::Zoom;

pub(crate) struct App {
    pub(crate) source: Source,
    pub(crate) images: image::ImageStore,
    pub(crate) seen_images_version: u64,
    pub(crate) raw: bool,
    pub(crate) doc: Document,
    pub(crate) stream: Option<Receiver<String>>,
    pub(crate) stick_bottom: bool,
    pub(crate) name: String,
    pub(crate) max_width: usize,
    pub(crate) view: Viewport,
    pub(crate) search: SearchState,
    pub(crate) zoom: Option<Zoom>,
    /// Zoom de imagem (`i`): um diagrama Mermaid na tela inteira.
    pub(crate) img_zoom: Option<imgview::ImgZoom>,
    /// O zoom de imagem já enviou algo ao terminal (limpar ao sair).
    pub(crate) zoom_sent: bool,
    pub(crate) notice: Option<String>,
    pub(crate) open_table: Option<usize>,
    pub(crate) pending_osc: Option<String>,
    pub(crate) cli: cli::Cli,
    /// Link selecionado: (linha renderizada, índice em `Rendered.links`).
    pub(crate) link_at: Option<(usize, usize)>,
    /// URL externa esperando confirmação (`y`).
    pub(crate) pending_open: Option<String>,
    pub(crate) back: Vec<browse::Back>,
    /// Seletor de arquivos aberto (`mdv <pasta>`); enquanto existe, ocupa a tela.
    pub(crate) picker: Option<picker::Picker>,
    pub(crate) pending_jump: Option<browse::Jump>,
    /// Ids gráficos (Kitty) já enviados ao terminal.
    pub(crate) sent_graphics: HashSet<u32>,
    /// Sequências Kitty ainda não escritas no terminal.
    pub(crate) pending_gfx: String,
}

impl App {
    pub(crate) fn new(source: Source, md: String, max_width: usize, cli: &cli::Cli) -> Self {
        let name = match &source {
            Source::File(p) => render::sanitize(&p.file_name().map_or_else(
                || p.display().to_string(),
                |n| n.to_string_lossy().into_owned(),
            ))
            .into_owned(),
            Source::Stdin => "stdin".to_string(),
        };
        App {
            images: image_store(&source, cli, false),
            seen_images_version: 0,
            raw: false,
            doc: Document::new(md, DocOpts::from_cli(cli)),
            stream: None,
            stick_bottom: false,
            source,
            name,
            max_width: render::clamp_width(max_width),
            view: Viewport::default(),
            search: SearchState::default(),
            zoom: None,
            img_zoom: None,
            zoom_sent: false,
            notice: None,
            open_table: cli.table.map(|n| n - 1),
            pending_osc: None,
            cli: cli.clone(),
            link_at: None,
            pending_open: None,
            back: Vec::new(),
            picker: None,
            pending_jump: None,
            sent_graphics: HashSet::new(),
            pending_gfx: String::new(),
        }
    }

    /// Passa a receber o texto aos poucos (`--follow`).
    pub(crate) fn with_stream(mut self, rx: Receiver<String>) -> Self {
        self.stream = Some(rx);
        // Com o texto ainda chegando, um bloco mermaid pode estar incompleto: só gera no fim.
        self.images.set_mermaid_hold(true);
        self
    }

    /// Acrescenta o que chegou do stdin; só passa a acompanhar o fim se o leitor já estava nele.
    pub(crate) fn drain_stream(&mut self) {
        let Some(rx) = &self.stream else { return };
        let was_at_bottom = self.view.scroll >= self.view.max_scroll(self.total_lines());
        let mut got = false;
        let mut finished = false;
        let mut chunks = String::new();
        loop {
            match rx.try_recv() {
                Ok(chunk) => {
                    chunks.push_str(&chunk);
                    got = true;
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    finished = true;
                    break;
                }
            }
        }
        if !chunks.is_empty() {
            self.doc.append_md(&chunks);
        }
        self.stop_stream_for_memory(mem::check().err());
        if finished {
            self.stream = None;
            self.images.set_mermaid_hold(false);
            got = true;
        }
        if got {
            self.stick_bottom |= was_at_bottom;
            self.doc.dirty = true;
        }
    }

    /// Para de acumular o stdin quando a memória passou do limite (`--follow`).
    pub(crate) fn stop_stream_for_memory(&mut self, over: Option<mem::MemoryLimit>) {
        if let Some(e) = over {
            if self.stream.take().is_some() {
                self.images.set_mermaid_hold(false);
                self.notice = Some(format!("leitura interrompida: {e}"));
                self.doc.dirty = true;
            }
        }
    }

    /// Depois de redesenhar, leva a tela ao fim se o leitor estava acompanhando o fim.
    pub(crate) fn apply_stick_bottom(&mut self) {
        if self.stick_bottom {
            self.view.scroll = self.view.max_scroll(self.total_lines());
            self.stick_bottom = false;
        }
    }

    /// Copia o bloco de código sob a linha de referência (`y`).
    pub(crate) fn copy_code(&mut self) {
        let top = self.view.scroll;
        let janela = self.window();
        let achado = self
            .doc
            .rendered
            .code_blocks
            .iter()
            .find(|c| c.start <= top && top < c.end)
            .or_else(|| {
                self.doc
                    .rendered
                    .code_blocks
                    .iter()
                    .find(|c| c.start < janela.end && c.end > janela.start)
            })
            .map(|c| c.text.clone());
        let Some(text) = achado else {
            self.notice = Some("nenhum bloco de código".to_string());
            return;
        };
        self.copy_text(&text);
    }

    /// Copia o Markdown cru da seção da linha de referência (`Y`).
    pub(crate) fn copy_section(&mut self) {
        if self.raw || self.doc.opts.mode != cli::Mode::Markdown {
            self.notice = Some("seção só existe no Markdown renderizado".to_string());
            return;
        }
        let Some(idx) = render::section_at(&self.doc.rendered.outline, self.view.scroll) else {
            self.notice = Some("nenhuma seção aqui".to_string());
            return;
        };
        match render::section_source(&self.doc.md, &self.doc.rendered, idx) {
            Some(text) => self.copy_text(&text),
            None => self.notice = Some("não consegui recortar a seção".to_string()),
        }
    }

    /// Prepara o OSC 52 (escrito pelo `event_loop`) ou avisa que o texto é grande demais.
    pub(crate) fn copy_text(&mut self, text: &str) {
        match clip::osc52(text) {
            Ok(seq) => {
                self.pending_osc = Some(seq);
                let n = text.lines().count();
                self.notice = Some(format!(
                    "copiado: {n} linha{}",
                    if n == 1 { "" } else { "s" }
                ));
            }
            Err(bytes) => {
                self.notice = Some(format!(
                    "grande demais ({} KiB), nada copiado",
                    bytes / 1024
                ));
            }
        }
    }

    pub(crate) fn rerender(&mut self, w: usize) {
        self.doc.render(w, self.raw, &self.images);
        if self.link_at.is_some_and(|(l, id)| {
            l >= self.doc.rendered.lines.len() || id >= self.doc.rendered.links.len()
        }) {
            self.link_at = None;
        }
        self.search.refresh(&self.doc.rendered.lines);
        self.view.scroll = self
            .view
            .scroll
            .min(self.view.max_scroll(self.total_lines()));
        let n = self.doc.rendered.tables.len();
        if self.zoom.as_ref().is_some_and(|z| z.table >= n) {
            self.zoom = None;
        } else if let Some(z) = &mut self.zoom {
            z.drawn = None;
        }
        self.sync_img_zoom();
        self.queue_graphics();
    }

    /// Trabalho de fundo a cada tique: um download de imagem terminou, então redesenha com ela.
    pub(crate) fn poll_background(&mut self) {
        let version = self.images.version();
        if version != self.seen_images_version {
            self.seen_images_version = version;
            self.doc.dirty = true;
        }
    }

    /// Relê o arquivo; se a leitura falhar, mantém o documento atual.
    pub(crate) fn reload(&mut self) {
        let Source::File(p) = &self.source else {
            return;
        };
        if let Ok(md) = read_markdown_file(p) {
            self.doc.set_md(md);
        }
    }

    /// `w`: alterna entre código quebrado na largura e código sem quebra, rolável com `h`/`l`.
    pub(crate) fn toggle_code_wrap(&mut self) {
        self.doc.opts.code_scroll = match self.doc.opts.code_scroll {
            None => Some(0),
            Some(_) => None,
        };
        self.doc.dirty = true;
    }

    /// Vai para a coluna `to` do código (limitada ao fim do código mais largo).
    pub(crate) fn set_code_scroll(&mut self, to: usize) {
        let Some(cur) = self.doc.opts.code_scroll else {
            self.notice = Some("w: código sem quebra (h/l rolam)".into());
            return;
        };
        let max = self.doc.rendered.code_overflow;
        let next = to.min(max);
        if next != cur {
            self.doc.opts.code_scroll = Some(next);
            self.doc.dirty = true;
        } else if max == 0 {
            self.notice = Some("nenhum código largo aqui".into());
        }
    }

    pub(crate) fn scroll_code(&mut self, step: usize, forward: bool) {
        let Some(cur) = self.doc.opts.code_scroll else {
            self.set_code_scroll(0);
            return;
        };
        let to = if forward {
            cur.saturating_add(step)
        } else {
            cur.saturating_sub(step)
        };
        self.set_code_scroll(to);
    }

    /// Retorna `true` quando o usuário pediu para sair.
    pub(crate) fn on_key(&mut self, k: KeyEvent) -> bool {
        if self.picker.is_some() {
            if k.modifiers.contains(KeyModifiers::CONTROL) && k.code == KeyCode::Char('c') {
                return true;
            }
            return self.on_picker_key(k);
        }
        if k.modifiers.contains(KeyModifiers::CONTROL) && k.code == KeyCode::Char('c') {
            return true;
        }
        if self.img_zoom.is_some() {
            self.on_img_zoom_key(k.code);
            return false;
        }
        if self.zoom.is_some() {
            self.on_zoom_key(k.code);
            return false;
        }
        if self.pending_open.is_some() {
            self.confirm_open(k.code == KeyCode::Char('y'));
            return false;
        }
        self.notice = None;
        if let Some(buf) = self.search.input.take() {
            self.on_search_key(k.code, buf);
            return false;
        }

        let height = self.view.height;
        let page = isize::try_from(height.saturating_sub(1).max(1)).unwrap_or(isize::MAX);
        let half = isize::try_from((height / 2).max(1)).unwrap_or(isize::MAX);
        match k.code {
            KeyCode::Char('q') => return true,
            KeyCode::Esc => {
                if self.link_at.is_some() {
                    self.link_at = None;
                } else if self.search.query.is_empty() {
                    return true;
                } else {
                    self.search.clear();
                }
            }
            KeyCode::Tab => self.step_link(true),
            KeyCode::BackTab => self.step_link(false),
            KeyCode::Enter if self.link_at.is_some() => self.follow_link(),
            KeyCode::Backspace | KeyCode::Char('H') => self.go_back(),
            KeyCode::Char('j') | KeyCode::Down | KeyCode::Enter => self.scroll_by(1),
            KeyCode::Char('k') | KeyCode::Up => self.scroll_by(-1),
            KeyCode::Char(' ' | 'f') | KeyCode::PageDown => self.scroll_by(page),
            KeyCode::Char('b') | KeyCode::PageUp => self.scroll_by(-page),
            KeyCode::Char('d') => self.scroll_by(half),
            KeyCode::Char('u') => self.scroll_by(-half),
            KeyCode::Char('g') | KeyCode::Home => self.scroll_to(0),
            KeyCode::Char('G') | KeyCode::End => self.scroll_to(usize::MAX),
            KeyCode::Char(']') => {
                let top = self.view.scroll;
                let alvo = self
                    .doc
                    .rendered
                    .headings
                    .iter()
                    .copied()
                    .find(|&h| h > top);
                if let Some(h) = alvo {
                    self.scroll_to(h);
                }
            }
            KeyCode::Char('[') => {
                let top = self.view.scroll;
                let alvo = self
                    .doc
                    .rendered
                    .headings
                    .iter()
                    .rev()
                    .copied()
                    .find(|&h| h < top);
                if let Some(h) = alvo {
                    self.scroll_to(h);
                }
            }
            KeyCode::Char('/') => self.search.input = Some(String::new()),
            KeyCode::Char('n') => self.step_match(true),
            KeyCode::Char('N') => self.step_match(false),
            KeyCode::Char('r') => self.reload(),
            KeyCode::Char('v') if self.doc.opts.mode == cli::Mode::Json => {
                let current = if self.doc.opts.json_view == cli::JsonView::Auto {
                    data::auto_view_of_text(&self.doc.md, self.doc.opts.json_path.as_deref())
                } else {
                    self.doc.opts.json_view
                };
                self.doc.opts.json_view = data::next_view(current);
                self.doc.dirty = true;
            }
            KeyCode::Char('t') => self.open_visible_table(),
            KeyCode::Char('i') => self.open_visible_mermaid(),
            KeyCode::Char('y') => self.copy_code(),
            KeyCode::Char('Y') => self.copy_section(),
            KeyCode::Char('w') => self.toggle_code_wrap(),
            KeyCode::Char('l') | KeyCode::Right => self.scroll_code(8, true),
            KeyCode::Char('h') | KeyCode::Left => self.scroll_code(8, false),
            KeyCode::Char('0') => self.set_code_scroll(0),
            KeyCode::Char('$') => self.set_code_scroll(usize::MAX),
            KeyCode::Char('m') => {
                self.raw = !self.raw;
                self.doc.dirty = true;
            }
            _ => {}
        }
        false
    }

    /// Renderiza antes de desenhar, para que as imagens gráficas cheguem ao terminal
    /// **antes** dos placeholders que as mostram.
    pub(crate) fn prepare(&mut self, screen_width: u16) {
        let w = self.text_width(screen_width);
        if self.doc.needs_render(w) {
            self.rerender(w);
        }
    }

    /// Enfileira as imagens do documento atual que o terminal ainda não recebeu.
    pub(crate) fn queue_graphics(&mut self) {
        for g in &self.doc.rendered.graphics {
            if self.sent_graphics.insert(g.id) {
                self.pending_gfx
                    .push_str(&graphics::transmit(g.id, &g.png, g.cols, g.rows));
            }
        }
    }
}
