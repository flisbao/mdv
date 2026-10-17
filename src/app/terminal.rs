//! Terminal: laço de eventos, sequências de controle e restauração ao sair.

use super::*;
use ratatui::crossterm::terminal;

/// Intervalo entre verificações em segundo plano (download de imagem terminado).
pub(crate) const POLL_INTERVAL: Duration = Duration::from_millis(250);

/// Intervalo entre verificações com texto chegando por stdin (`--follow`).
pub(crate) const STREAM_POLL_INTERVAL: Duration = Duration::from_millis(80);

pub(crate) fn run_tui(mut app: App) -> Result<(), Failure> {
    let mut term = ratatui::init();
    let res = event_loop(&mut term, &mut app);
    if !app.sent_graphics.is_empty() || app.zoom_sent {
        emit_terminal_control(graphics::DELETE_ALL);
    }
    ratatui::restore();
    res.map_err(|e| Failure::Runtime(format!("erro no terminal: {e}")))
}

/// Escreve uma sequência de controle direto no terminal (OSC 52 para cópia, APC para Kitty).
pub(crate) fn emit_terminal_control(seq: &str) {
    let mut out = io::stdout();
    let _ = out.write_all(seq.as_bytes());
    let _ = out.flush();
}

/// Largura de uma célula em pixels (8 se o terminal não informa o tamanho da janela).
pub(crate) fn cell_px_width() -> f64 {
    match terminal::window_size() {
        Ok(ws) if ws.width > 0 && ws.columns > 0 => f64::from(ws.width) / f64::from(ws.columns),
        _ => 8.0,
    }
}

pub(crate) fn event_loop(term: &mut DefaultTerminal, app: &mut App) -> io::Result<()> {
    loop {
        let size = term.size()?;
        app.prepare(size.width);
        app.prepare_img_zoom(size.width, size.height, cell_px_width());
        if !app.pending_gfx.is_empty() {
            emit_terminal_control(&std::mem::take(&mut app.pending_gfx));
        }
        term.draw(|f| app.draw(f))?;
        let interval = if app.stream.is_some() {
            STREAM_POLL_INTERVAL
        } else {
            POLL_INTERVAL
        };
        if event::poll(interval)? {
            if let Event::Key(k) = event::read()? {
                if k.kind == KeyEventKind::Press && app.on_key(k) {
                    return Ok(());
                }
                if let Some(seq) = app.pending_osc.take() {
                    emit_terminal_control(&seq);
                }
            }
        } else {
            app.drain_stream();
            app.poll_background();
            if let Some(seq) = app.pending_osc.take() {
                emit_terminal_control(&seq);
            }
        }
    }
}
