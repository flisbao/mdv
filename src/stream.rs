//! Entrada incremental (stdin em streaming).

use std::io::Read;
use std::sync::mpsc::{channel, Receiver, Sender};

#[derive(Default)]
pub struct Utf8Accum {
    pending: Vec<u8>,
}

impl Utf8Accum {
    pub fn new() -> Self {
        Self::default()
    }

    /// Devolve o texto completo disponível; sobra de até 3 bytes fica para o próximo `push`.
    pub fn push(&mut self, bytes: &[u8]) -> String {
        self.pending.extend_from_slice(bytes);
        let mut out = String::new();
        loop {
            match std::str::from_utf8(&self.pending) {
                Ok(s) => {
                    out.push_str(s);
                    self.pending.clear();
                    return out;
                }
                Err(e) => {
                    let valid = e.valid_up_to();
                    out.push_str(std::str::from_utf8(&self.pending[..valid]).unwrap_or(""));
                    if let Some(n) = e.error_len() {
                        out.push('\u{FFFD}');
                        self.pending.drain(..valid + n);
                    } else {
                        self.pending.drain(..valid);
                        return out;
                    }
                }
            }
        }
    }
}

/// Lê stdin em outra thread e envia os pedaços de texto; para ao atingir `max_bytes`.
pub fn spawn_stdin_reader(max_bytes: usize) -> Receiver<String> {
    let (tx, rx) = channel();
    std::thread::spawn(move || pump(std::io::stdin().lock(), max_bytes, &tx));
    rx
}

/// Copia `reader` para `tx` até o fim ou até `max_bytes`; se ainda sobrava entrada, avisa no texto.
fn pump(mut reader: impl Read, max_bytes: usize, tx: &Sender<String>) {
    let (mut buf, mut acc, mut total) = ([0u8; 8192], Utf8Accum::new(), 0usize);
    while let Ok(n) = reader.read(&mut buf) {
        if n == 0 {
            break;
        }
        total += n;
        let chunk = acc.push(&buf[..n.min(max_bytes.saturating_sub(total - n))]);
        if !chunk.is_empty() && tx.send(chunk).is_err() {
            break;
        }
        if total >= max_bytes {
            // Só é truncamento se ainda chega mais alguma coisa.
            if total > max_bytes || reader.read(&mut buf).is_ok_and(|n| n > 0) {
                let _ = tx.send(format!("\n\n*(entrada truncada em {} MiB)*\n", max_bytes / (1024 * 1024)));
            }
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf8_dividido_entre_chunks() {
        let mut a = Utf8Accum::new();
        let bytes = "é".as_bytes(); // 2 bytes
        assert_eq!(a.push(&bytes[..1]), "");
        assert_eq!(a.push(&bytes[1..]), "é");
    }

    #[test]
    fn entrada_acima_do_teto_avisa_que_foi_truncada() {
        let (tx, rx) = channel();
        pump(std::io::Cursor::new(vec![b'a'; 3 * 1024 * 1024]), 2 * 1024 * 1024, &tx);
        drop(tx);
        let all: String = rx.iter().collect();
        assert!(all.ends_with("*(entrada truncada em 2 MiB)*\n"), "{:?}", &all[all.len() - 40..]);
        let (tx, rx) = channel();
        pump(std::io::Cursor::new(vec![b'a'; 2 * 1024 * 1024]), 2 * 1024 * 1024, &tx);
        drop(tx);
        assert!(!rx.iter().collect::<String>().contains("truncada"), "exatamente no teto não trunca");
    }

    #[test]
    fn utf8_invalido_vira_substituto_sem_travar() {
        let mut a = Utf8Accum::new();
        assert_eq!(a.push(b"a\xFFb"), "a\u{FFFD}b");
    }
}
