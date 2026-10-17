//! Escritor de ZIP mínimo, sem compressão (método "stored"). Basta para DOCX e ODT: o texto é
//! pequeno, as imagens (PNG) já vêm comprimidas e o ODT exige `mimetype` sem compressão.

const LOCAL_SIG: u32 = 0x0403_4b50;
const CENTRAL_SIG: u32 = 0x0201_4b50;
const END_SIG: u32 = 0x0605_4b50;
/// 1980-01-01, o menor valor de data válido no ZIP.
const DOS_DATE: u16 = 0x0021;
/// Teto de entradas (o formato sem ZIP64 aceita 65 535).
pub const MAX_ENTRIES: usize = 60_000;

struct Entry {
    name: String,
    crc: u32,
    size: u32,
    offset: u32,
}

#[derive(Default)]
pub struct ZipWriter {
    buf: Vec<u8>,
    entries: Vec<Entry>,
}

/// CRC-32 (polinômio 0xEDB88320), o do ZIP/PNG.
pub fn crc32(data: &[u8]) -> u32 {
    let mut table = [0u32; 256];
    for (i, slot) in table.iter_mut().enumerate() {
        let mut c = i as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
        }
        *slot = c;
    }
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc = table[((crc ^ u32::from(b)) & 0xFF) as usize] ^ (crc >> 8);
    }
    !crc
}

fn u16le(buf: &mut Vec<u8>, v: u16) {
    buf.extend_from_slice(&v.to_le_bytes());
}

fn u32le(buf: &mut Vec<u8>, v: u32) {
    buf.extend_from_slice(&v.to_le_bytes());
}

impl ZipWriter {
    pub fn new() -> Self {
        Self::default()
    }

    /// Acrescenta um arquivo. `name` deve ser um caminho fixo do programa (nunca vindo do documento).
    pub fn add(&mut self, name: &str, data: &[u8]) -> Result<(), String> {
        if self.entries.len() >= MAX_ENTRIES {
            return Err("arquivos demais dentro do pacote".to_string());
        }
        let size = u32::try_from(data.len()).map_err(|_| "arquivo grande demais para o pacote".to_string())?;
        let offset = u32::try_from(self.buf.len()).map_err(|_| "pacote grande demais".to_string())?;
        let crc = crc32(data);
        u32le(&mut self.buf, LOCAL_SIG);
        u16le(&mut self.buf, 20); // versão necessária
        u16le(&mut self.buf, 0); // flags
        u16le(&mut self.buf, 0); // método: stored
        u16le(&mut self.buf, 0); // hora
        u16le(&mut self.buf, DOS_DATE);
        u32le(&mut self.buf, crc);
        u32le(&mut self.buf, size);
        u32le(&mut self.buf, size);
        u16le(&mut self.buf, u16::try_from(name.len()).map_err(|_| "nome longo demais".to_string())?);
        u16le(&mut self.buf, 0); // extra
        self.buf.extend_from_slice(name.as_bytes());
        self.buf.extend_from_slice(data);
        self.entries.push(Entry { name: name.to_string(), crc, size, offset });
        Ok(())
    }

    /// Fecha o pacote (diretório central) e devolve os bytes.
    pub fn finish(mut self) -> Result<Vec<u8>, String> {
        let cd_start = u32::try_from(self.buf.len()).map_err(|_| "pacote grande demais".to_string())?;
        let mut cd = Vec::new();
        for e in &self.entries {
            u32le(&mut cd, CENTRAL_SIG);
            u16le(&mut cd, 20); // versão de criação
            u16le(&mut cd, 20); // versão necessária
            u16le(&mut cd, 0);
            u16le(&mut cd, 0);
            u16le(&mut cd, 0);
            u16le(&mut cd, DOS_DATE);
            u32le(&mut cd, e.crc);
            u32le(&mut cd, e.size);
            u32le(&mut cd, e.size);
            u16le(&mut cd, e.name.len() as u16);
            u16le(&mut cd, 0); // extra
            u16le(&mut cd, 0); // comentário
            u16le(&mut cd, 0); // disco
            u16le(&mut cd, 0); // atributos internos
            u32le(&mut cd, 0); // atributos externos
            u32le(&mut cd, e.offset);
            cd.extend_from_slice(e.name.as_bytes());
        }
        let cd_size = u32::try_from(cd.len()).map_err(|_| "pacote grande demais".to_string())?;
        self.buf.extend_from_slice(&cd);
        let n = u16::try_from(self.entries.len()).map_err(|_| "arquivos demais".to_string())?;
        u32le(&mut self.buf, END_SIG);
        u16le(&mut self.buf, 0);
        u16le(&mut self.buf, 0);
        u16le(&mut self.buf, n);
        u16le(&mut self.buf, n);
        u32le(&mut self.buf, cd_size);
        u32le(&mut self.buf, cd_start);
        u16le(&mut self.buf, 0);
        Ok(self.buf)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc32_do_valor_de_referencia() {
        // Valor clássico: CRC-32 de "123456789".
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(crc32(b""), 0);
    }

    #[test]
    fn zip_tem_assinaturas_e_contagem() {
        let mut z = ZipWriter::new();
        z.add("a.txt", b"oi").unwrap();
        z.add("dir/b.txt", b"mundo").unwrap();
        let bytes = z.finish().unwrap();
        assert_eq!(&bytes[..4], b"PK\x03\x04");
        let end = &bytes[bytes.len() - 22..];
        assert_eq!(&end[..4], b"PK\x05\x06");
        assert_eq!(u16::from_le_bytes([end[10], end[11]]), 2, "duas entradas");
        assert!(bytes.windows(5).any(|w| w == b"mundo"));
    }

    #[test]
    fn entradas_demais_sao_recusadas() {
        let mut z = ZipWriter::new();
        for _ in 0..MAX_ENTRIES {
            z.add("x", b"").unwrap();
        }
        assert!(z.add("x", b"").is_err());
    }
}
