//! Modelo da mensagem lida do `.msg`: propriedades da raiz, destinatários, anexos e mensagens embutidas.

use crate::container::{self, tags::*, Cfb, Propriedades};
use crate::container::{TIPO_BINARIO, TIPO_STRING8};
use crate::{Budget, MAX_ATTACHMENTS, MAX_EMBED_DEPTH, MAX_RECIPIENTS};

const CABECALHO_RAIZ: usize = 32;
const CABECALHO_EMBUTIDA: usize = 24;
const CABECALHO_ITEM: usize = 8;
const PREFIXO_DESTINATARIO: &str = "__recip_version1.0_#";
const PREFIXO_ANEXO: &str = "__attach_version1.0_#";
const CP_PADRAO: u32 = 1252;
const DEST_PARA: i32 = 1;
const DEST_CC: i32 = 2;
const DEST_CCO: i32 = 3;
const METODO_EMBUTIDA: i32 = 5;

/// Mensagem lida (raiz ou embutida em anexo).
#[derive(Debug, Default)]
pub(crate) struct Message {
    pub subject: Option<String>,
    pub sender_name: Option<String>,
    pub sender_email: Option<String>,
    pub to: Vec<Recipient>,
    pub cc: Vec<Recipient>,
    pub display_to: Option<String>,
    pub display_cc: Option<String>,
    pub submit_time: Option<u64>,
    pub delivery_time: Option<u64>,
    pub transport_headers: Option<String>,
    pub class: Option<String>,
    pub html: Option<Vec<u8>>,
    pub rtf: Option<Vec<u8>>,
    pub text: Option<String>,
    pub codepage: u32,
    pub html_codepage: u32,
    pub attachments: Vec<Attachment>,
    pub recipients_truncated: bool,
    pub attachments_truncated: bool,
}

#[derive(Debug, Default)]
pub(crate) struct Recipient {
    pub name: Option<String>,
    pub email: Option<String>,
}

#[derive(Debug, Default)]
pub(crate) struct Attachment {
    pub name: String,
    pub size: Option<u32>,
    pub mime: Option<String>,
    pub hidden: bool,
    pub embedded: Option<Box<Message>>,
}

/// Lê um `.msg` inteiro. Bytes que não são OLE2 e contêineres truncados viram `Err`, nunca pânico.
pub(crate) fn read(bytes: &[u8], budget: &dyn Budget) -> Result<Message, String> {
    let mut cfb = container::abre(bytes)?;
    le_mensagem(&mut cfb, "/", 0, budget)
}

/// Lê uma mensagem em `dir` (`/` para a raiz). `profundidade` 0 é a raiz; embutidas vão até `MAX_EMBED_DEPTH`.
fn le_mensagem(cfb: &mut Cfb<'_>, dir: &str, profundidade: usize, budget: &dyn Budget) -> Result<Message, String> {
    let cabecalho = if profundidade == 0 { CABECALHO_RAIZ } else { CABECALHO_EMBUTIDA };
    let mut m = Message::default();
    // As propriedades fixas são consumidas antes da recursão para não manter dois streams grandes vivos.
    let props = Propriedades::ler(cfb, dir, cabecalho)?;
    m.codepage = props
        .inteiro(MESSAGE_CODEPAGE)
        .or_else(|| props.inteiro(INTERNET_CPID))
        .map_or(CP_PADRAO, |v| v as u32);
    m.html_codepage = props.inteiro(INTERNET_CPID).map_or(m.codepage, |v| v as u32);
    m.submit_time = props.tempo(CLIENT_SUBMIT_TIME);
    m.delivery_time = props.tempo(MESSAGE_DELIVERY_TIME);
    drop(props);

    let cp = m.codepage;
    m.subject = container::texto(cfb, dir, SUBJECT, cp)?;
    m.sender_name = primeiro(cfb, dir, cp, &[SENDER_NAME, SENT_REPRESENTING_NAME])?;
    m.sender_email = match primeiro(cfb, dir, cp, &[SENDER_SMTP_ADDRESS, SENT_REPRESENTING_SMTP_ADDRESS])? {
        Some(email) => Some(email),
        None => container::texto(cfb, dir, SENDER_EMAIL_ADDRESS, cp)?.filter(|e| !eh_endereco_exchange(e)),
    };
    m.display_to = container::texto(cfb, dir, DISPLAY_TO, cp)?;
    m.display_cc = container::texto(cfb, dir, DISPLAY_CC, cp)?;
    m.transport_headers = container::texto(cfb, dir, TRANSPORT_MESSAGE_HEADERS, cp)?;
    m.class = container::texto(cfb, dir, MESSAGE_CLASS, cp)?;
    m.text = container::texto(cfb, dir, BODY, cp)?;
    m.html = corpo_html(cfb, dir)?;
    m.rtf = container::binario(cfb, dir, RTF_COMPRESSED, TIPO_BINARIO)?;

    let (destinatarios, trunc_dest) = container::filhos_numerados(cfb, dir, PREFIXO_DESTINATARIO, MAX_RECIPIENTS)?;
    m.recipients_truncated = trunc_dest;
    for caminho in &destinatarios {
        budget.check()?;
        let (tipo, destinatario) = le_destinatario(cfb, caminho, cp)?;
        match tipo {
            DEST_CC => m.cc.push(destinatario),
            DEST_CCO => {}
            _ => m.to.push(destinatario),
        }
    }

    let (anexos, trunc_anx) = container::filhos_numerados(cfb, dir, PREFIXO_ANEXO, MAX_ATTACHMENTS)?;
    m.attachments_truncated = trunc_anx;
    for caminho in &anexos {
        budget.check()?;
        m.attachments.push(le_anexo(cfb, caminho, cp, profundidade, budget)?);
    }
    Ok(m)
}

/// Primeiro texto encontrado entre os ids dados.
fn primeiro(cfb: &mut Cfb<'_>, dir: &str, cp: u32, ids: &[u16]) -> Result<Option<String>, String> {
    for &id in ids {
        if let Some(texto) = container::texto(cfb, dir, id, cp)? {
            return Ok(Some(texto));
        }
    }
    Ok(None)
}

/// `/O=` é endereço interno do Exchange, não um e-mail.
fn eh_endereco_exchange(endereco: &str) -> bool {
    endereco.as_bytes().get(..3).is_some_and(|p| p.eq_ignore_ascii_case(b"/O="))
}

/// Corpo HTML: bytes brutos (`0102` ou `001E`, decodificados depois com a codepage do HTML) ou texto `001F` em UTF-8.
fn corpo_html(cfb: &mut Cfb<'_>, dir: &str) -> Result<Option<Vec<u8>>, String> {
    if let Some(bytes) = container::binario(cfb, dir, HTML, TIPO_BINARIO)? {
        return Ok(Some(bytes));
    }
    if let Some(bytes) = container::binario(cfb, dir, HTML, TIPO_STRING8)? {
        return Ok(Some(bytes));
    }
    Ok(container::texto_unicode(cfb, dir, HTML)?.map(String::into_bytes))
}

fn le_destinatario(cfb: &mut Cfb<'_>, caminho: &str, cp: u32) -> Result<(i32, Recipient), String> {
    let props = Propriedades::ler(cfb, caminho, CABECALHO_ITEM)?;
    let tipo = props.inteiro(RECIPIENT_TYPE).unwrap_or(DEST_PARA);
    drop(props);
    let name = container::texto(cfb, caminho, DISPLAY_NAME, cp)?;
    let email = match container::texto(cfb, caminho, SMTP_ADDRESS, cp)? {
        Some(email) => Some(email),
        None => container::texto(cfb, caminho, EMAIL_ADDRESS, cp)?.filter(|e| !eh_endereco_exchange(e)),
    };
    Ok((tipo, Recipient { name, email }))
}

fn le_anexo(
    cfb: &mut Cfb<'_>,
    caminho: &str,
    cp: u32,
    profundidade: usize,
    budget: &dyn Budget,
) -> Result<Attachment, String> {
    let props = Propriedades::ler(cfb, caminho, CABECALHO_ITEM)?;
    let size = props.inteiro(ATTACH_SIZE).map(|v| v as u32);
    let metodo = props.inteiro(ATTACH_METHOD);
    let hidden = props.booleano(ATTACHMENT_HIDDEN).unwrap_or(false);
    drop(props);
    let name = primeiro(cfb, caminho, cp, &[ATTACH_LONG_FILENAME, ATTACH_FILENAME, DISPLAY_NAME])?
        .unwrap_or_else(|| "anexo".to_string());
    let mime = container::texto(cfb, caminho, ATTACH_MIME_TAG, cp)?;
    let embedded = if metodo == Some(METODO_EMBUTIDA) && profundidade < MAX_EMBED_DEPTH {
        let objeto = container::junta(caminho, &container::nome_propriedade(ATTACH_DATA, container::TIPO_OBJETO));
        if cfb.is_storage(&objeto) {
            Some(Box::new(le_mensagem(cfb, &objeto, profundidade + 1, budget)?))
        } else {
            None
        }
    } else {
        None
    };
    Ok(Attachment { name, size, mime, hidden, embedded })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{tags, AttachSpec, MsgBuilder};
    use crate::Unlimited;

    const RTF_EXEMPLO: &str = "2d 00 00 00 2b 00 00 00 4c 5a 46 75 f1 c5 c7 a7 03 00 0a 00 72 63 70 67 31 32 35 42 32 0a f3 20 68 65 6c 09 00 20 62 77 05 b0 6c 64 7d 0a 80 0f a0";

    fn hex(s: &str) -> Vec<u8> {
        s.split_whitespace().map(|b| u8::from_str_radix(b, 16).unwrap()).collect()
    }

    fn ler(bytes: &[u8]) -> Message {
        read(bytes, &Unlimited).expect("leitura")
    }

    #[test]
    fn le_assunto_e_remetente_unicode() {
        let bytes = MsgBuilder::new()
            .string(tags::SUBJECT, "Relatório mensal")
            .string(tags::SENDER_NAME, "Ana")
            .string(tags::SENDER_SMTP_ADDRESS, "ana@ex.com")
            .build();
        let m = ler(&bytes);
        assert_eq!(m.subject.as_deref(), Some("Relatório mensal"));
        assert_eq!(m.sender_name.as_deref(), Some("Ana"));
        assert_eq!(m.sender_email.as_deref(), Some("ana@ex.com"));
    }

    #[test]
    fn string8_usa_codepage_da_mensagem() {
        let bytes = MsgBuilder::new()
            .string8(tags::SUBJECT, b"Relat\xf3rio")
            .int(tags::MESSAGE_CODEPAGE, 1252)
            .build();
        assert_eq!(ler(&bytes).subject.as_deref(), Some("Relatório"));
    }

    #[test]
    fn endereco_exchange_e_ignorado() {
        let bytes = MsgBuilder::new()
            .string(tags::SENDER_EMAIL_ADDRESS, "/O=EXCHANGELABS/OU=Exchange/CN=ana")
            .build();
        assert_eq!(ler(&bytes).sender_email, None);
    }

    #[test]
    fn destinatarios_separados_por_tipo() {
        let bytes = MsgBuilder::new()
            .recipient(1, "A", "a@ex.com")
            .recipient(1, "B", "b@ex.com")
            .recipient(2, "C", "c@ex.com")
            .recipient(3, "D", "d@ex.com")
            .build();
        let m = ler(&bytes);
        assert_eq!(m.to.len(), 2);
        assert_eq!(m.cc.len(), 1);
        assert_eq!(m.to[1].email.as_deref(), Some("b@ex.com"));
        assert_eq!(m.cc[0].name.as_deref(), Some("C"));
    }

    #[test]
    fn anexos_listados_com_tamanho_e_mime() {
        let bytes = MsgBuilder::new()
            .attachment(AttachSpec {
                name: "a.xlsx".into(),
                data: vec![0; 2048],
                mime: Some("application/x".into()),
                ..Default::default()
            })
            .build();
        let m = ler(&bytes);
        assert_eq!(m.attachments.len(), 1);
        assert_eq!(m.attachments[0].name, "a.xlsx");
        assert_eq!(m.attachments[0].size, Some(2048));
        assert_eq!(m.attachments[0].mime.as_deref(), Some("application/x"));
    }

    #[test]
    fn anexo_sem_nome_usa_padrao() {
        let bytes = MsgBuilder::new().attachment(AttachSpec::default()).build();
        assert_eq!(ler(&bytes).attachments[0].name, "anexo");
    }

    #[test]
    fn mensagem_embutida_e_lida_recursivamente() {
        let bytes = MsgBuilder::new()
            .attachment(AttachSpec {
                name: "msg.msg".into(),
                embedded: Some(MsgBuilder::new().string(tags::SUBJECT, "Original")),
                ..Default::default()
            })
            .build();
        let m = ler(&bytes);
        let embutida = m.attachments[0].embedded.as_ref().expect("embutida");
        assert_eq!(embutida.subject.as_deref(), Some("Original"));
    }

    fn cadeia(nivel: usize) -> MsgBuilder {
        let b = MsgBuilder::new().string(tags::SUBJECT, &format!("N{nivel}"));
        if nivel < 4 {
            b.attachment(AttachSpec {
                name: "n.msg".into(),
                embedded: Some(cadeia(nivel + 1)),
                ..Default::default()
            })
        } else {
            b
        }
    }

    #[test]
    fn embutida_alem_da_profundidade_maxima_nao_e_lida() {
        assert_eq!(MAX_EMBED_DEPTH, 3);
        let m = ler(&cadeia(0).build());
        let n1 = m.attachments[0].embedded.as_ref().expect("profundidade 1");
        let n2 = n1.attachments[0].embedded.as_ref().expect("profundidade 2");
        let n3 = n2.attachments[0].embedded.as_ref().expect("profundidade 3");
        assert_eq!(n3.subject.as_deref(), Some("N3"));
        assert!(n3.attachments[0].embedded.is_none());
    }

    #[test]
    fn corpos_html_rtf_e_texto_sao_lidos() {
        let bytes = MsgBuilder::new()
            .binary(tags::HTML, b"<p>x</p>")
            .binary(tags::RTF_COMPRESSED, &hex(RTF_EXEMPLO))
            .string(tags::BODY, "x")
            .build();
        let m = ler(&bytes);
        assert_eq!(m.html.as_deref(), Some(&b"<p>x</p>"[..]));
        assert_eq!(m.rtf, Some(hex(RTF_EXEMPLO)));
        assert_eq!(m.text.as_deref(), Some("x"));
    }

    #[test]
    fn muitos_destinatarios_param_no_teto() {
        let mut b = MsgBuilder::new();
        for i in 0..=MAX_RECIPIENTS {
            b = b.recipient(1, &format!("R{i}"), &format!("r{i}@ex.com"));
        }
        let m = ler(&b.build());
        assert_eq!(m.to.len(), MAX_RECIPIENTS);
        assert!(m.recipients_truncated);
    }

    #[test]
    fn bytes_que_nao_sao_ole2_dao_erro_claro() {
        let erro = read(b"Assunto: oi", &Unlimited).expect_err("deve falhar");
        assert!(erro.contains("não é um arquivo .msg válido"), "{erro}");
    }

    #[test]
    fn msg_truncado_nao_entra_em_panico() {
        let bytes = MsgBuilder::new()
            .string(tags::SUBJECT, "Assunto")
            .recipient(1, "A", "a@ex.com")
            .attachment(AttachSpec {
                name: "dados.bin".into(),
                data: vec![7; 1000],
                ..Default::default()
            })
            .attachment(AttachSpec {
                name: "m.msg".into(),
                embedded: Some(MsgBuilder::new().string(tags::SUBJECT, "Dentro")),
                ..Default::default()
            })
            .build();
        for n in (0..bytes.len()).step_by(512) {
            let _ = read(&bytes[..n], &Unlimited);
        }
    }
}
