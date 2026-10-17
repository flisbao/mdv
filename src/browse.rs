//! Navegação na TUI: escolher links (`Tab`), seguir (`Enter`), voltar (`Backspace`/`H`) e abrir URLs.

use crate::nav::{self, Target};
use crate::picker::{self, Picker};
use crate::render::{self, link_id};
use crate::{file_mtime, image_store, read_markdown_file, App, Source};
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;
use std::path::PathBuf;
use std::time::SystemTime;
use unicode_width::UnicodeWidthStr;

/// Teto da pilha de "voltar".
const MAX_BACK: usize = 100;

/// Para onde voltar.
pub enum Back {
    /// O seletor de arquivos, com a consulta e a seleção de antes.
    Picker(Box<Picker>),
    /// Um documento, na linha renderizada que estava no topo, e o modo em que estava aberto.
    File { path: PathBuf, line: usize, mode: crate::cli::Mode },
}

/// Posição a aplicar depois do próximo desenho (só então o documento novo tem linhas).
pub enum Jump {
    Anchor(String),
    Line(usize),
}

impl App {
    /// Cada link visível, na ordem do texto: (linha renderizada, índice do link).
    fn link_occurrences(&self) -> Vec<(usize, usize)> {
        let mut out = Vec::new();
        for &i in &self.display {
            let mut last = None;
            for sp in &self.rendered.lines[i].spans {
                if let Some(id) = link_id(&sp.style) {
                    if last != Some(id) {
                        out.push((i, id));
                    }
                    last = Some(id);
                } else if !sp.content.trim().is_empty() {
                    last = None;
                }
            }
        }
        out
    }

    /// `Tab`/`Shift-Tab`: seleciona o próximo/anterior link, rolando até ele.
    pub fn step_link(&mut self, forward: bool) {
        let occ = self.link_occurrences();
        if occ.is_empty() {
            self.notice = Some("nenhum link neste documento".to_string());
            return;
        }
        let from = self.link_at.unwrap_or_else(|| {
            if forward {
                (self.ref_line(), 0)
            } else {
                (self.line_at(self.view.scroll + self.view.height), 0)
            }
        });
        let started = self.link_at.is_some();
        let pick = if forward {
            occ.iter().find(|&&o| if started { o > from } else { o >= from }).or(occ.first())
        } else {
            occ.iter().rev().find(|&&o| o < from).or(occ.last())
        };
        let Some(&(line, id)) = pick else { return };
        self.link_at = Some((line, id));
        let pos = self.pos_of(line);
        if pos < self.view.scroll || pos >= self.view.scroll + self.view.height.max(1) {
            self.scroll_to(pos.saturating_sub(self.view.height / 3));
        }
        let url = self.rendered.links.get(id).map_or("", String::as_str);
        self.notice = Some(format!("link: {}  (Enter abre, Esc cancela)", render::sanitize(url)));
    }

    /// `Enter` com um link selecionado: segue o destino.
    pub fn follow_link(&mut self) {
        let Some((_, id)) = self.link_at else { return };
        let Some(url) = self.rendered.links.get(id).cloned() else { return };
        let base = match &self.source {
            Source::File(p) => p.parent().map(|d| {
                if d.as_os_str().is_empty() { PathBuf::from(".") } else { d.to_path_buf() }
            }),
            Source::Stdin => None,
        };
        match nav::resolve(&url, base.as_deref()) {
            Target::Anchor(a) => self.jump_anchor(&a),
            Target::External(u) => {
                self.notice = Some(format!("abrir no navegador: {u}  (y confirma, outra tecla cancela)"));
                self.pending_open = Some(u);
            }
            Target::Local { path, anchor } => {
                self.open_file(path, anchor.map(Jump::Anchor), true, crate::cli::Mode::Markdown)
            }
            Target::Unsupported(why) => self.notice = Some(format!("não abri o link: {why}")),
        }
    }

    /// Resposta à confirmação de `pending_open`.
    pub fn confirm_open(&mut self, yes: bool) {
        let Some(url) = self.pending_open.take() else { return };
        if !yes {
            self.notice = Some("cancelado".to_string());
            return;
        }
        self.notice = Some(match nav::open_external(&url) {
            Ok(()) => format!("aberto: {url}"),
            Err(e) => e,
        });
    }

    fn jump_anchor(&mut self, anchor: &str) {
        let titles: Vec<&str> = self.rendered.outline.iter().map(|e| e.title.as_str()).collect();
        match nav::heading_for_anchor(&titles, anchor) {
            Some(i) => {
                let line = self.rendered.outline[i].line;
                // Seção dobrada: abre as dobras que escondem o título antes de rolar.
                self.folded.clear();
                self.recompute_display();
                self.scroll_to_line(line);
            }
            None => self.notice = Some(format!("âncora não encontrada: #{}", render::sanitize(anchor))),
        }
    }

    /// Troca o documento por `path`. Com `push`, guarda o atual em "voltar".
    pub fn open_file(&mut self, path: PathBuf, jump: Option<Jump>, push: bool, mode: crate::cli::Mode) {
        let md = match read_markdown_file(&path) {
            Ok(md) => md,
            Err(e) => {
                self.notice = Some(
                    render::sanitize(&format!("não consegui abrir {}: {e}", path.display())).into_owned(),
                );
                return;
            }
        };
        if push {
            if let Source::File(cur) = &self.source {
                if self.back.len() >= MAX_BACK {
                    self.back.remove(0);
                }
                self.back.push(Back::File { path: cur.clone(), line: self.ref_line(), mode: self.doc.mode });
            }
        }
        self.name = render::sanitize(
            &path
                .file_name()
                .map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned()),
        )
        .into_owned();
        self.mtime = file_mtime(&path);
        self.source = Source::File(path);
        self.images = image_store(&self.source, &self.cli, false);
        self.seen_images_version = 0;
        self.md = md;
        self.raw = false;
        self.doc.mode = mode;
        self.changed.clear();
        self.marks.clear();
        self.folded.clear();
        self.link_at = None;
        self.search.clear();
        self.zoom = None;
        self.view.scroll = 0;
        self.pending_jump = jump;
        self.dirty = true;
    }

    /// `Backspace`/`H`: volta ao documento anterior.
    pub fn go_back(&mut self) {
        match self.back.pop() {
            Some(Back::File { path, line, mode }) => self.open_file(path, Some(Jump::Line(line)), false, mode),
            Some(Back::Picker(p)) => {
                self.picker = Some(*p);
                self.link_at = None;
            }
            None => self.notice = Some("nada para voltar".to_string()),
        }
    }

    /// Aplica a posição pedida na troca de documento; chamado depois de renderizar.
    pub fn apply_pending_jump(&mut self) {
        match self.pending_jump.take() {
            Some(Jump::Anchor(a)) => self.jump_anchor(&a),
            Some(Jump::Line(l)) => self.scroll_to_line(l),
            None => {}
        }
    }

    /// Destaque do link selecionado (invertido) e retirada da marca de navegação.
    pub fn decorate_links(&self, line: &mut ratatui::text::Line<'static>) {
        let sel = self.link_at.map(|(_, id)| id);
        for sp in &mut line.spans {
            if sel.is_some() && link_id(&sp.style) == sel {
                sp.style = sp.style.add_modifier(Modifier::REVERSED);
            }
        }
        render::strip_link_marks(line);
    }
}

impl App {
    /// Teclas no seletor: digitar filtra, ↑/↓ escolhem, Enter abre, Esc sai.
    pub fn on_picker_key(&mut self, k: KeyEvent) -> bool {
        let Some(p) = self.picker.as_mut() else { return false };
        match k.code {
            KeyCode::Esc => return true,
            KeyCode::Up => p.step(-1),
            KeyCode::Down => p.step(1),
            KeyCode::Char('p') if k.modifiers.contains(KeyModifiers::CONTROL) => p.step(-1),
            KeyCode::Char('n') if k.modifiers.contains(KeyModifiers::CONTROL) => p.step(1),
            KeyCode::PageUp => p.step(-10),
            KeyCode::PageDown => p.step(10),
            KeyCode::Backspace => p.pop(),
            KeyCode::Char(c) if !k.modifiers.contains(KeyModifiers::CONTROL) => p.push(c),
            KeyCode::Enter => {
                if let Some(path) = p.current().map(|e| e.path.clone()) {
                    let keep = Box::new(p.clone());
                    self.picker = None;
                    self.open_file(path, None, false, crate::cli::Mode::Markdown);
                    // Só empilha se o arquivo abriu (senão o seletor continua na tela).
                    if matches!(self.source, Source::File(_)) {
                        self.back.push(Back::Picker(keep));
                    } else {
                        self.picker = Some(*keep);
                    }
                }
            }
            _ => {}
        }
        false
    }

    pub fn draw_picker(&self, f: &mut Frame) {
        let Some(p) = &self.picker else { return };
        let area = f.area();
        let [body, status] = Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(area);
        let rows = usize::from(body.height).saturating_sub(2).max(1);
        let top = p.selected.saturating_sub(rows - 1);
        let now = SystemTime::now();
        let width = usize::from(body.width);
        let mut lines = vec![
            Line::from(Span::styled(
                format!(" {}", render::sanitize(&p.dir.display().to_string())),
                Style::default().fg(Color::Rgb(121, 192, 255)),
            )),
            Line::from(format!(" > {}█", p.query)),
        ];
        for (n, &i) in p.filtered.iter().enumerate().skip(top).take(rows) {
            let e = &p.entries[i];
            let age = picker::age(e.mtime, now);
            let name_w = width.saturating_sub(age.width() + 4).max(1);
            let mut name = e.name.clone();
            while name.width() > name_w {
                name.pop();
            }
            let pad = width.saturating_sub(name.width() + age.width() + 3);
            let text = format!(" {name}{}{age} ", " ".repeat(pad));
            let style = if n == p.selected {
                Style::default().add_modifier(ratatui::style::Modifier::REVERSED)
            } else {
                Style::default()
            };
            lines.push(Line::from(Span::styled(text, style)));
        }
        if p.filtered.is_empty() {
            lines.push(Line::from(Span::styled(
                " nenhum arquivo .md encontrado",
                Style::default().fg(Color::Rgb(110, 118, 129)),
            )));
        }
        f.render_widget(Paragraph::new(lines), body);
        let bar = Style::default().bg(Color::Rgb(59, 66, 82)).fg(Color::Rgb(229, 233, 240));
        let more = if p.truncated { "  (lista cortada)" } else { "" };
        let text = self.notice.clone().map_or_else(
            || {
                format!(
            " {}/{} arquivos{more}   digite para filtrar  ↑/↓ escolher  Enter abre  Esc sai",
            p.filtered.len(),
            p.entries.len()
                )
            },
            |n| format!(" {n}"),
        );
        f.render_widget(Paragraph::new(Line::from(text)).style(bar), status);
    }
}
