//! Leitura da entrada com teto de bytes (o `mdv-tools` nunca lê além dele).

use std::io::{self, Read};

/// Maior documento aceito, em bytes (o mesmo teto do `mdv`).
pub const MAX_INPUT_BYTES: u64 = 16 * 1024 * 1024;

/// Lê UTF-8 de `reader`, falhando se passar de `max` bytes (sem ler além de `max + 1`).
pub fn read_limited(reader: impl Read, max: u64) -> io::Result<String> {
    let mut bytes = Vec::new();
    reader.take(max + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > max {
        let msg = format!("entrada maior que {} MiB", max / (1024 * 1024));
        return Err(io::Error::new(io::ErrorKind::InvalidData, msg));
    }
    String::from_utf8(bytes).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io;

    #[test]
    fn aceita_exatamente_o_teto() {
        assert_eq!(read_limited(&b"abcd"[..], 4).unwrap(), "abcd");
    }

    #[test]
    fn rejeita_acima_do_teto_sem_ler_alem() {
        assert!(read_limited(&b"abcde"[..], 4).is_err());
        let mut infinito = io::repeat(b'a');
        assert!(read_limited(&mut infinito, 1024).is_err());
    }

    #[test]
    fn rejeita_utf8_invalido() {
        let e = read_limited(&[0xff, 0xfe][..], 16).unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::InvalidData);
    }
}
