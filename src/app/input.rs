//! Teclas do modo normal: `action_for` traduz a tecla em `Action` e `App::apply` a executa.

use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Action {
    Quit,
    ScrollLines(isize),
    /// `+1` uma página abaixo, `-1` acima.
    ScrollPage(isize),
    ScrollHalf(isize),
    Top,
    Bottom,
    NextHeading,
    PrevHeading,
    SearchOpen,
    SearchNext,
    SearchPrev,
    Reload,
    NextLink,
    PrevLink,
    FollowLink,
    Back,
    ToggleRaw,
    ToggleCodeWrap,
    ScrollCode {
        step: usize,
        forward: bool,
    },
    /// `0` ou `usize::MAX` (início ou fim da linha de código).
    CodeScrollTo(usize),
    CycleJsonView,
    OpenTableZoom,
    OpenImageZoom,
    CopyCode,
    CopySection,
    /// Limpa o link selecionado, depois a busca e só então sai.
    EscNormal,
}

/// Tecla do modo normal (`Modal::None`) → ação. `link_selected` distingue o `Enter` que segue o link do que rola.
pub(crate) fn action_for(k: KeyEvent, link_selected: bool) -> Option<Action> {
    use Action::*;
    Some(match k.code {
        KeyCode::Char('q') => Quit,
        KeyCode::Esc => EscNormal,
        KeyCode::Tab => NextLink,
        KeyCode::BackTab => PrevLink,
        KeyCode::Enter if link_selected => FollowLink,
        KeyCode::Backspace | KeyCode::Char('H') => Back,
        KeyCode::Char('j') | KeyCode::Down | KeyCode::Enter => ScrollLines(1),
        KeyCode::Char('k') | KeyCode::Up => ScrollLines(-1),
        KeyCode::Char(' ' | 'f') | KeyCode::PageDown => ScrollPage(1),
        KeyCode::Char('b') | KeyCode::PageUp => ScrollPage(-1),
        KeyCode::Char('d') => ScrollHalf(1),
        KeyCode::Char('u') => ScrollHalf(-1),
        KeyCode::Char('g') | KeyCode::Home => Top,
        KeyCode::Char('G') | KeyCode::End => Bottom,
        KeyCode::Char(']') => NextHeading,
        KeyCode::Char('[') => PrevHeading,
        KeyCode::Char('/') => SearchOpen,
        KeyCode::Char('n') => SearchNext,
        KeyCode::Char('N') => SearchPrev,
        KeyCode::Char('r') => Reload,
        KeyCode::Char('v') => CycleJsonView,
        KeyCode::Char('t') => OpenTableZoom,
        KeyCode::Char('i') => OpenImageZoom,
        KeyCode::Char('y') => CopyCode,
        KeyCode::Char('Y') => CopySection,
        KeyCode::Char('w') => ToggleCodeWrap,
        KeyCode::Char('l') | KeyCode::Right => ScrollCode {
            step: 8,
            forward: true,
        },
        KeyCode::Char('h') | KeyCode::Left => ScrollCode {
            step: 8,
            forward: false,
        },
        KeyCode::Char('0') => CodeScrollTo(0),
        KeyCode::Char('$') => CodeScrollTo(usize::MAX),
        KeyCode::Char('m') => ToggleRaw,
        _ => return None,
    })
}

impl App {
    /// Executa a ação. Devolve `true` quando o usuário pediu para sair.
    pub(crate) fn apply(&mut self, action: Action) -> bool {
        let height = self.view.height;
        let page = isize::try_from(height.saturating_sub(1).max(1)).unwrap_or(isize::MAX);
        let half = isize::try_from((height / 2).max(1)).unwrap_or(isize::MAX);
        match action {
            Action::Quit => return true,
            Action::EscNormal => {
                if self.link_at.is_some() {
                    self.link_at = None;
                } else if self.search.query.is_empty() {
                    return true;
                } else {
                    self.search.clear();
                }
            }
            Action::NextLink => self.step_link(true),
            Action::PrevLink => self.step_link(false),
            Action::FollowLink => self.follow_link(),
            Action::Back => self.go_back(),
            Action::ScrollLines(n) => self.scroll_by(n),
            Action::ScrollPage(n) => self.scroll_by(n * page),
            Action::ScrollHalf(n) => self.scroll_by(n * half),
            Action::Top => self.scroll_to(0),
            Action::Bottom => self.scroll_to(usize::MAX),
            Action::NextHeading => {
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
            Action::PrevHeading => {
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
            Action::SearchOpen => self.modal = Modal::Search(String::new()),
            Action::SearchNext => self.step_match(true),
            Action::SearchPrev => self.step_match(false),
            Action::Reload => self.reload(),
            Action::CycleJsonView => {
                let yaml = matches!(self.doc.opts.doc, cli::Doc::Yaml { .. });
                if let Some((path, view)) = self.doc.opts.doc.data_view_mut() {
                    let current = match *view {
                        cli::JsonView::Auto if yaml => crate::yaml_view::auto_view_of_text(&self.doc.md, path.as_deref()),
                        cli::JsonView::Auto => data::auto_view_of_text(&self.doc.md, path.as_deref()),
                        v => v,
                    };
                    *view = data::next_view(current);
                    self.doc.dirty = true;
                }
            }
            Action::OpenTableZoom => self.open_visible_table(),
            Action::OpenImageZoom => self.open_visible_mermaid(),
            Action::CopyCode => self.copy_code(),
            Action::CopySection => self.copy_section(),
            Action::ToggleCodeWrap => self.toggle_code_wrap(),
            Action::ScrollCode { step, forward } => self.scroll_code(step, forward),
            Action::CodeScrollTo(to) => self.set_code_scroll(to),
            Action::ToggleRaw => {
                self.raw = !self.raw;
                self.doc.dirty = true;
            }
        }
        false
    }
}
