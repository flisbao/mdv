//! Montador de `.msg` para testes: grava o layout MS-CFB/MS-OXMSG com os ids de `tags`.
//! Só existe em testes (ou com a feature `testutil`); os dados das fixtures são fictícios.

use crate::container::{junta, nome_propriedade, PROPRIEDADES};
use crate::container::{TIPO_BINARIO, TIPO_BOOLEANO, TIPO_LONG, TIPO_OBJETO, TIPO_STRING8, TIPO_TEMPO, TIPO_UNICODE};
use std::io::{Cursor, Write};

pub use crate::container::tags;

type Escritor = cfb::CompoundFile<Cursor<Vec<u8>>>;

/// Anexo a montar. Com `embedded`, o anexo é uma mensagem embutida (`ATTACH_METHOD = 5`).
#[derive(Default)]
pub struct AttachSpec {
    pub name: String,
    pub data: Vec<u8>,
    pub mime: Option<String>,
    pub hidden: bool,
    pub embedded: Option<MsgBuilder>,
}

/// Mensagem a gravar. Tamanho e contagem dos anexos saem sozinhos de `data`.
#[derive(Default)]
pub struct MsgBuilder {
    props: Vec<Prop>,
    recipients: Vec<Vec<Prop>>,
    attachments: Vec<AttachSpec>,
}

enum Valor {
    Texto(String),
    Texto8(Vec<u8>),
    Binario(Vec<u8>),
    Fixo(u16, [u8; 8]),
}

struct Prop {
    id: u16,
    valor: Valor,
}

fn texto(id: u16, v: &str) -> Prop {
    Prop { id, valor: Valor::Texto(v.to_string()) }
}

fn binario(id: u16, v: &[u8]) -> Prop {
    Prop { id, valor: Valor::Binario(v.to_vec()) }
}

fn fixo(id: u16, tipo: u16, bytes: [u8; 8]) -> Prop {
    Prop { id, valor: Valor::Fixo(tipo, bytes) }
}

fn fixo_int(id: u16, v: i32) -> Prop {
    let mut b = [0u8; 8];
    b[..4].copy_from_slice(&v.to_le_bytes());
    fixo(id, TIPO_LONG, b)
}

fn fixo_bool(id: u16, v: bool) -> Prop {
    let mut b = [0u8; 8];
    b[0] = u8::from(v);
    fixo(id, TIPO_BOOLEANO, b)
}

impl MsgBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Texto UTF-16LE (`001F`).
    pub fn string(mut self, id: u16, v: &str) -> Self {
        self.props.push(texto(id, v));
        self
    }

    /// Bytes na codepage da mensagem (`001E`).
    pub fn string8(mut self, id: u16, bytes: &[u8]) -> Self {
        self.props.push(Prop { id, valor: Valor::Texto8(bytes.to_vec()) });
        self
    }

    /// Bytes brutos (`0102`).
    pub fn binary(mut self, id: u16, v: &[u8]) -> Self {
        self.props.push(binario(id, v));
        self
    }

    /// `PT_LONG`, na tabela de propriedades fixas.
    pub fn int(mut self, id: u16, v: i32) -> Self {
        self.props.push(fixo_int(id, v));
        self
    }

    /// `PT_SYSTIME` (FILETIME), na tabela de propriedades fixas.
    pub fn time(mut self, id: u16, filetime: u64) -> Self {
        self.props.push(fixo(id, TIPO_TEMPO, filetime.to_le_bytes()));
        self
    }

    /// `PT_BOOLEAN`, na tabela de propriedades fixas.
    pub fn boolean(mut self, id: u16, v: bool) -> Self {
        self.props.push(fixo_bool(id, v));
        self
    }

    /// Destinatário: `kind` 1 = Para, 2 = Cc, 3 = Cco.
    pub fn recipient(mut self, kind: i32, name: &str, email: &str) -> Self {
        self.recipients.push(vec![
            fixo_int(tags::RECIPIENT_TYPE, kind),
            texto(tags::DISPLAY_NAME, name),
            texto(tags::SMTP_ADDRESS, email),
        ]);
        self
    }

    pub fn attachment(mut self, a: AttachSpec) -> Self {
        self.attachments.push(a);
        self
    }

    /// Serializa o `.msg` completo em memória.
    pub fn build(&self) -> Vec<u8> {
        let mut cfb = cfb::CompoundFile::create(Cursor::new(Vec::new())).expect("contêiner de teste");
        escreve_mensagem(&mut cfb, "/", self, 32);
        cfb.flush().expect("flush do contêiner de teste");
        cfb.into_inner().into_inner()
    }
}

fn escreve_mensagem(cfb: &mut Escritor, dir: &str, m: &MsgBuilder, cabecalho: usize) {
    if dir != "/" {
        cfb.create_storage(dir).expect("storage de teste");
    }
    grava_bytes(cfb, &junta(dir, "__nameid_version1.0"), &[]);
    // Raiz: contagens nos offsets 16 e 20; embutida: nos offsets 8 e 12.
    let mut cabec = vec![0u8; cabecalho];
    let (rec, anx) = (m.recipients.len() as u32, m.attachments.len() as u32);
    let (off_rec, off_anx) = if cabecalho == 32 { (16, 20) } else { (8, 12) };
    cabec[off_rec..off_rec + 4].copy_from_slice(&rec.to_le_bytes());
    cabec[off_anx..off_anx + 4].copy_from_slice(&anx.to_le_bytes());
    escreve_props(cfb, dir, &m.props, cabec);
    for (i, props) in m.recipients.iter().enumerate() {
        let pasta = junta(dir, &format!("__recip_version1.0_#{i:08X}"));
        cfb.create_storage(&pasta).expect("destinatário de teste");
        escreve_props(cfb, &pasta, props, vec![0u8; 8]);
    }
    for (i, a) in m.attachments.iter().enumerate() {
        let pasta = junta(dir, &format!("__attach_version1.0_#{i:08X}"));
        cfb.create_storage(&pasta).expect("anexo de teste");
        escreve_anexo(cfb, &pasta, a);
    }
}

fn escreve_anexo(cfb: &mut Escritor, pasta: &str, a: &AttachSpec) {
    let metodo = if a.embedded.is_some() { 5 } else { 1 };
    let mut props = vec![
        texto(tags::ATTACH_LONG_FILENAME, &a.name),
        texto(tags::ATTACH_FILENAME, &a.name),
        fixo_int(tags::ATTACH_SIZE, a.data.len() as i32),
        fixo_int(tags::ATTACH_METHOD, metodo),
    ];
    if let Some(mime) = &a.mime {
        props.push(texto(tags::ATTACH_MIME_TAG, mime));
    }
    if a.hidden {
        props.push(fixo_bool(tags::ATTACHMENT_HIDDEN, true));
    }
    if a.embedded.is_none() {
        props.push(binario(tags::ATTACH_DATA, &a.data));
    }
    escreve_props(cfb, pasta, &props, vec![0u8; 8]);
    if let Some(m) = &a.embedded {
        let objeto = junta(pasta, &nome_propriedade(tags::ATTACH_DATA, TIPO_OBJETO));
        escreve_mensagem(cfb, &objeto, m, 24);
    }
}

/// Grava as propriedades: variáveis viram streams; fixas entram na tabela após o cabeçalho.
fn escreve_props(cfb: &mut Escritor, dir: &str, props: &[Prop], mut tabela: Vec<u8>) {
    for p in props {
        match &p.valor {
            Valor::Fixo(tipo, v) => {
                tabela.extend_from_slice(&((u32::from(p.id) << 16) | u32::from(*tipo)).to_le_bytes());
                tabela.extend_from_slice(&6u32.to_le_bytes());
                tabela.extend_from_slice(v);
            }
            Valor::Texto(s) => {
                let mut bytes: Vec<u8> = s.encode_utf16().flat_map(u16::to_le_bytes).collect();
                bytes.extend_from_slice(&[0, 0]);
                grava_bytes(cfb, &junta(dir, &nome_propriedade(p.id, TIPO_UNICODE)), &bytes);
            }
            Valor::Texto8(b) => grava_bytes(cfb, &junta(dir, &nome_propriedade(p.id, TIPO_STRING8)), b),
            Valor::Binario(b) => grava_bytes(cfb, &junta(dir, &nome_propriedade(p.id, TIPO_BINARIO)), b),
        }
    }
    grava_bytes(cfb, &junta(dir, PROPRIEDADES), &tabela);
}

fn grava_bytes(cfb: &mut Escritor, caminho: &str, bytes: &[u8]) {
    let mut fluxo = cfb.create_stream(caminho).expect("stream de teste");
    fluxo.write_all(bytes).expect("escrita de teste");
}
