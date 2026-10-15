//! Pergunte à IA (opt-in): `?` envia a seção atual ao comando de `MDV_AI_CMD` e mostra a resposta.
//! Sem rede embutida: é só um subprocesso (sem shell) que lê a seção no stdin e escreve a resposta
//! no stdout, com timeout e teto de bytes, no molde do `mmdc`.

use crate::{render, App};
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Style};
use ratatui::text::Line;
use ratatui::widgets::Paragraph;
use ratatui::Frame;
use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::Arc;
use std::time::{Duration, Instant};
use unicode_width::UnicodeWidthStr;

/// Teto da seção enviada ao comando.
pub const MAX_SEND_BYTES: usize = 64 * 1024;
/// Teto da resposta lida.
pub const MAX_REPLY_BYTES: usize = 256 * 1024;
pub const AI_TIMEOUT: Duration = Duration::from_secs(60);

/// Tempo extra, depois que o filho sai, para as threads de leitura terminarem. Um neto que herdou
/// stdout/stderr mantém o pipe aberto; passado o prazo, desistimos da leitura.
const READ_GRACE: Duration = Duration::from_secs(1);

/// `MDV_AI_CMD` dividido por espaços (sem aspas nem shell): programa e argumentos.
pub fn parse_command(raw: &str) -> Option<Vec<String>> {
    let v: Vec<String> = raw.split_whitespace().map(str::to_string).collect();
    (!v.is_empty()).then_some(v)
}

/// Corta `s` em no máximo `max` bytes, num limite de caractere. `true` se cortou.
pub fn clip_bytes(s: &str, max: usize) -> (&str, bool) {
    if s.len() <= max {
        return (s, false);
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    (&s[..end], true)
}

/// Roda `cmd` com `input` no stdin; devolve o stdout (até `max_out` bytes) ou o motivo da falha.
/// Atalho sem cancelamento: só os testes o usam (a TUI chama `run_cancelable`).
#[cfg(test)]
pub fn run(cmd: &[String], input: &str, timeout: Duration, max_out: usize) -> Result<String, String> {
    run_cancelable(cmd, input, timeout, max_out, &AtomicBool::new(false))
}

/// Como `run`, mas `cancel` em `true` mata o filho e devolve `Err("cancelado")`.
pub fn run_cancelable(
    cmd: &[String],
    input: &str,
    timeout: Duration,
    max_out: usize,
    cancel: &AtomicBool,
) -> Result<String, String> {
    let (prog, args) = cmd.split_first().ok_or("comando vazio")?;
    let mut child = Command::new(prog)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("não consegui executar {prog}: {e}"))?;

    let mut stdin = child.stdin.take().ok_or("sem stdin")?;
    let data = input.as_bytes().to_vec();
    std::thread::spawn(move || {
        // O comando pode sair sem ler tudo (`BrokenPipe`): não é erro.
        let _ = stdin.write_all(&data);
    });

    let overflow = Arc::new(AtomicBool::new(false));
    let mut stdout = child.stdout.take().ok_or("sem stdout")?;
    let flag = Arc::clone(&overflow);
    let (out_tx, out_rx) = mpsc::channel::<Vec<u8>>();
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = (&mut stdout).take(max_out as u64 + 1).read_to_end(&mut buf);
        if buf.len() > max_out {
            flag.store(true, Ordering::Relaxed);
        }
        let _ = out_tx.send(buf);
    });
    let mut stderr = child.stderr.take().ok_or("sem stderr")?;
    let (err_tx, err_rx) = mpsc::channel::<String>();
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = (&mut stderr).take(2048).read_to_end(&mut buf);
        // Continua lendo (e descartando) o resto: fechar o pipe mataria o filho com SIGPIPE.
        let _ = std::io::copy(&mut stderr, &mut std::io::sink());
        let _ = err_tx.send(String::from_utf8_lossy(&buf).into_owned());
    });

    let start = Instant::now();
    let status = loop {
        if cancel.load(Ordering::Relaxed) {
            let _ = child.kill();
            let _ = child.wait();
            return Err("cancelado".to_string());
        }
        if overflow.load(Ordering::Relaxed) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!("resposta maior que {} KiB, descartada", max_out / 1024));
        }
        match child.try_wait().map_err(|e| e.to_string())? {
            Some(s) => break s,
            None if start.elapsed() > timeout => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("tempo esgotado ({}s)", timeout.as_secs()));
            }
            None => std::thread::sleep(Duration::from_millis(20)),
        }
    };
    let Ok(out) = out_rx.recv_timeout(READ_GRACE) else {
        return Err("o comando deixou um processo preso à saída; resposta descartada".to_string());
    };
    let err = err_rx.recv_timeout(READ_GRACE).unwrap_or_default();
    if out.len() > max_out {
        return Err(format!("resposta maior que {} KiB, descartada", max_out / 1024));
    }
    if !status.success() {
        let first = err.lines().find(|l| !l.trim().is_empty()).unwrap_or("falha desconhecida");
        return Err(format!("{prog}: {}", first.trim()));
    }
    Ok(String::from_utf8_lossy(&out).into_owned())
}

/// Painel com a resposta (ou o aviso de que ainda está consultando).
pub struct AiPanel {
    label: String,
    rx: Option<Receiver<Result<String, String>>>,
    reply: Option<Result<String, String>>,
    drawn: Option<(usize, render::Rendered)>,
    scroll: usize,
    /// Sinaliza à thread da consulta que o painel foi fechado: ela mata o subprocesso.
    cancel: Arc<AtomicBool>,
}

impl Drop for AiPanel {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

impl AiPanel {
    #[cfg(test)]
    pub fn has_reply(&self) -> bool {
        self.reply.is_some()
    }

    #[cfg(test)]
    pub fn reply_text(&self) -> Option<&str> {
        self.reply.as_ref().and_then(|r| r.as_deref().ok())
    }
}

impl App {
    /// `?`: envia a seção atual ao comando configurado.
    pub fn ask_ai(&mut self) {
        let Some(cmd) = self.ai_cmd.clone() else {
            self.notice = Some("defina MDV_AI_CMD (ex.: MDV_AI_CMD='claude -p') para usar a IA".to_string());
            return;
        };
        if self.raw || self.doc.mode != crate::cli::Mode::Markdown {
            self.notice = Some("a IA só lê seções do Markdown renderizado".to_string());
            return;
        }
        let Some(idx) = render::section_at(&self.rendered.outline, self.ref_line()) else {
            self.notice = Some("nenhuma seção aqui".to_string());
            return;
        };
        let Some(section) = render::section_source(&self.md, &self.rendered, idx) else {
            self.notice = Some("não consegui recortar a seção".to_string());
            return;
        };
        let (sent, _) = clip_bytes(&section, MAX_SEND_BYTES);
        let sent = sent.to_string();
        let (tx, rx) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = Arc::clone(&cancel);
        std::thread::spawn(move || {
            let _ = tx.send(run_cancelable(&cmd, &sent, AI_TIMEOUT, MAX_REPLY_BYTES, &worker_cancel));
        });
        let label = self.ai_cmd.as_ref().and_then(|c| c.first()).cloned().unwrap_or_default();
        self.ai = Some(AiPanel { label, rx: Some(rx), reply: None, drawn: None, scroll: 0, cancel });
    }

    /// Recebe a resposta quando ela chega (chamado a cada tique).
    pub fn poll_ai(&mut self) {
        let Some(p) = &mut self.ai else { return };
        let Some(rx) = &p.rx else { return };
        match rx.try_recv() {
            Ok(r) => {
                p.reply = Some(r);
                p.rx = None;
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => {
                p.reply = Some(Err("o comando terminou sem resposta".to_string()));
                p.rx = None;
            }
        }
    }

    pub fn on_ai_key(&mut self, code: ratatui::crossterm::event::KeyCode) {
        use ratatui::crossterm::event::KeyCode;
        let h = self.view.height.max(2);
        let Some(p) = &mut self.ai else { return };
        match code {
            KeyCode::Esc | KeyCode::Char('q' | '?') => self.ai = None,
            KeyCode::Char('j') | KeyCode::Down | KeyCode::Enter => p.scroll = p.scroll.saturating_add(1),
            KeyCode::Char('k') | KeyCode::Up => p.scroll = p.scroll.saturating_sub(1),
            KeyCode::Char(' ' | 'f') | KeyCode::PageDown => p.scroll = p.scroll.saturating_add(h - 1),
            KeyCode::Char('b') | KeyCode::PageUp => p.scroll = p.scroll.saturating_sub(h - 1),
            KeyCode::Char('g') | KeyCode::Home => p.scroll = 0,
            KeyCode::Char('G') | KeyCode::End => p.scroll = usize::MAX,
            _ => {}
        }
    }

    pub fn draw_ai(&mut self, f: &mut Frame) {
        let [body, status] = Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(f.area());
        self.view.height = usize::from(body.height);
        let width = render::clamp_width(usize::from(body.width).saturating_sub(2));
        let Some(p) = &mut self.ai else { return };
        let (lines, label): (Vec<Line<'static>>, String) = match &p.reply {
            None => (vec![Line::from(" consultando…")], format!("consultando {}…  Esc cancela", p.label)),
            Some(r) => {
                if p.drawn.as_ref().is_none_or(|(w, _)| *w != width) {
                    let rendered = match r {
                        // O renderizador sanitiza o texto: a resposta nunca injeta controles no terminal.
                        Ok(text) => render::render(text, width),
                        Err(e) => render::error_rendered(e),
                    };
                    p.drawn = Some((width, rendered));
                }
                let all = p.drawn.as_ref().map(|(_, r)| r.lines.clone()).unwrap_or_default();
                let max = all.len().saturating_sub(self.view.height);
                p.scroll = p.scroll.min(max);
                let lines = all.into_iter().skip(p.scroll).take(self.view.height).collect();
                (lines, format!("resposta de {}  j/k rolar  Esc fecha", p.label))
            }
        };
        f.render_widget(Paragraph::new(lines), body);
        let bar = Style::default().bg(Color::Rgb(59, 66, 82)).fg(Color::Rgb(229, 233, 240));
        let w = usize::from(status.width);
        let text = if label.width() < w { format!(" {label}") } else { label };
        f.render_widget(Paragraph::new(Line::from(text)).style(bar), status);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cmd(s: &str) -> Vec<String> {
        parse_command(s).unwrap()
    }

    #[test]
    fn divide_por_espacos_e_ignora_vazio() {
        assert_eq!(cmd("claude  -p"), ["claude", "-p"]);
        assert!(parse_command("   ").is_none());
    }

    #[test]
    fn corta_em_limite_de_caractere() {
        assert_eq!(clip_bytes("aé", 2), ("a", true));
        assert_eq!(clip_bytes("abc", 10), ("abc", false));
    }

    #[cfg(unix)]
    #[test]
    fn cat_devolve_a_entrada() {
        let r = run(&cmd("cat"), "olá", Duration::from_secs(5), 1024).unwrap();
        assert_eq!(r, "olá");
    }

    #[cfg(unix)]
    #[test]
    fn timeout_mata_o_processo() {
        let t = Instant::now();
        let e = run(&cmd("sleep 5"), "", Duration::from_millis(200), 1024).unwrap_err();
        assert!(e.contains("tempo esgotado"), "{e}");
        assert!(t.elapsed() < Duration::from_secs(3));
    }

    #[cfg(unix)]
    #[test]
    fn resposta_acima_do_teto_e_descartada() {
        let t = Instant::now();
        let e = run(&cmd("yes"), "", Duration::from_secs(10), 4096).unwrap_err();
        assert!(e.contains("maior que"), "{e}");
        assert!(t.elapsed() < Duration::from_secs(5));
    }

    #[cfg(unix)]
    #[test]
    fn falha_mostra_a_primeira_linha_do_stderr() {
        let e = run(&cmd("ls /nao/existe/mdv"), "", Duration::from_secs(5), 1024).unwrap_err();
        assert!(e.starts_with("ls:"), "{e}");
    }

    #[test]
    fn programa_inexistente() {
        let e = run(&cmd("/nao/existe/mdv-ai"), "", Duration::from_secs(1), 10).unwrap_err();
        assert!(e.contains("não consegui executar"), "{e}");
    }

    #[cfg(unix)]
    #[test]
    fn cancelar_mata_o_processo() {
        let cancel = Arc::new(AtomicBool::new(false));
        let c2 = Arc::clone(&cancel);
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(150));
            c2.store(true, Ordering::Relaxed);
        });
        let t = Instant::now();
        let e = run_cancelable(&cmd("sleep 5"), "", Duration::from_secs(10), 1024, &cancel).unwrap_err();
        assert_eq!(e, "cancelado");
        assert!(t.elapsed() < Duration::from_secs(3), "{:?}", t.elapsed());
    }

    #[cfg(unix)]
    #[test]
    fn neto_preso_na_saida_nao_trava() {
        // O `sleep` em segundo plano herda o stdout e mantém o pipe aberto por 4 s.
        let c = vec!["sh".to_string(), "-c".to_string(), "sleep 4 & echo oi".to_string()];
        let t = Instant::now();
        let e = run(&c, "", Duration::from_secs(10), 1024).unwrap_err();
        assert!(e.contains("preso"), "{e}");
        assert!(t.elapsed() < Duration::from_secs(3), "{:?}", t.elapsed());
    }

    #[cfg(unix)]
    #[test]
    fn stderr_grande_nao_derruba_o_comando() {
        // ~33 KB no stderr (acima dos 2 KiB lidos) e mesmo assim sai com sucesso.
        let script = "i=0; while [ $i -lt 3000 ]; do echo abcdefghij >&2; i=$((i+1)); done; echo ok";
        let c = vec!["sh".to_string(), "-c".to_string(), script.to_string()];
        let r = run(&c, "", Duration::from_secs(10), 1024).unwrap();
        assert_eq!(r.trim(), "ok");
    }

    #[test]
    fn soltar_o_painel_sinaliza_cancelamento() {
        let cancel = Arc::new(AtomicBool::new(false));
        let p = AiPanel {
            label: String::new(),
            rx: None,
            reply: None,
            drawn: None,
            scroll: 0,
            cancel: Arc::clone(&cancel),
        };
        drop(p);
        assert!(cancel.load(Ordering::Relaxed));
    }
}
