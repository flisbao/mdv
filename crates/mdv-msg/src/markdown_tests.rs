//! Testes de `markdown.rs`: cabeçalho, corpo, embutidas, orçamento e escapes.

use crate::testutil::{tags, AttachSpec, MsgBuilder};
use crate::{to_markdown, Budget, Unlimited};
use std::cell::Cell;

/// Orçamento que estoura depois de `n` chamadas (de `check` ou `check_html`) e continua estourado.
struct Cota(Cell<usize>);

const MSG_COTA: &str = "limite de memória de teste";

impl Cota {
    fn gasta(&self) -> Result<(), String> {
        let restante = self.0.get();
        if restante == 0 {
            return Err(MSG_COTA.to_string());
        }
        self.0.set(restante - 1);
        Ok(())
    }
}

impl Budget for Cota {
    fn check(&self) -> Result<(), String> {
        self.gasta()
    }

    fn check_html(&self, _len: usize) -> Result<(), String> {
        self.gasta()
    }
}

fn md(bytes: &[u8]) -> String {
    to_markdown(bytes, &Unlimited).expect("conversão")
}

/// Empacota `raw` como MELA (sem compressão), o formato do `PR_RTF_COMPRESSED` nos testes.
fn mela(raw: &[u8]) -> Vec<u8> {
    let mut v = ((raw.len() + 12) as u32).to_le_bytes().to_vec();
    v.extend((raw.len() as u32).to_le_bytes());
    v.extend(b"MELA");
    v.extend(0u32.to_le_bytes());
    v.extend(raw);
    v
}

#[test]
fn cabecalho_completo() {
    let bytes = MsgBuilder::new()
        .string(tags::SUBJECT, "Assunto")
        .string(tags::SENDER_NAME, "Ana")
        .string(tags::SENDER_SMTP_ADDRESS, "ana@ex.com")
        .recipient(1, "Bia", "bia@ex.com")
        .recipient(1, "Caio", "")
        .recipient(2, "Duda", "duda@ex.com")
        .string(tags::TRANSPORT_MESSAGE_HEADERS, "Date: Tue, 21 Oct 2026 14:32:10 -0300\r\n")
        .string(tags::BODY, "corpo")
        .build();
    let saida = md(&bytes);
    let cabeca = saida.split("\n\n---\n\n").next().unwrap();
    assert_eq!(
        cabeca,
        "# Assunto\n\n- **De:** Ana \\<ana@ex.com\\>\n- **Para:** Bia \\<bia@ex.com\\>; Caio\n- **Cc:** Duda \\<duda@ex.com\\>\n- **Data:** 2026-10-21 14:32 (-03:00)"
    );
    assert!(saida.ends_with("corpo\n"));
}

#[test]
fn html_tem_prioridade_sobre_rtf_e_texto() {
    let bytes = MsgBuilder::new()
        .binary(tags::HTML, b"<p>de html</p>")
        .string(tags::BODY, "de texto")
        .build();
    let saida = md(&bytes);
    assert!(saida.contains("de html"));
    assert!(!saida.contains("de texto"));
}

#[test]
fn rtf_com_html_encapsulado_e_usado_sem_pr_html() {
    let rtf = br"{\rtf1\ansi\ansicpg1252\fromhtml1 {\*\htmltag64 <p>}via rtf{\*\htmltag72 </p>}}";
    let bytes = MsgBuilder::new().binary(tags::RTF_COMPRESSED, &mela(rtf)).build();
    assert!(md(&bytes).contains("via rtf"));
}

#[test]
fn texto_puro_quando_so_ha_body() {
    let bytes = MsgBuilder::new().string(tags::BODY, "linha 1\r\n# linha 2").build();
    assert!(md(&bytes).contains("linha 1\\\n\\# linha 2"));
}

#[test]
fn texto_puro_preserva_linhas_e_recuo() {
    let bytes = MsgBuilder::new().string(tags::BODY, "Att,\r\nAna\r\n    recuado").build();
    assert!(md(&bytes).contains("Att,\\\nAna\\\n\u{a0}\u{a0}\u{a0}\u{a0}recuado"));
}

#[test]
fn titulo_com_cerquilha_final_nao_vira_fechamento() {
    let bytes = MsgBuilder::new().string(tags::SUBJECT, "Pedido #").string(tags::BODY, "x").build();
    assert!(md(&bytes).starts_with("# Pedido \\#\n"));
}

#[test]
fn data_zero_conta_como_ausente() {
    let bytes = MsgBuilder::new()
        .time(tags::CLIENT_SUBMIT_TIME, 0)
        .time(tags::MESSAGE_DELIVERY_TIME, 0)
        .string(tags::BODY, "x")
        .build();
    assert!(!md(&bytes).contains("- **Data:**"));
}

#[test]
fn estouro_do_budget_aborta() {
    let bytes = MsgBuilder::new()
        .binary(tags::HTML, b"<p>ok html</p>")
        .string(tags::BODY, "texto")
        .build();
    let mut viu_erro = false;
    for n in 0..=200 {
        match to_markdown(&bytes, &Cota(Cell::new(n))) {
            Ok(saida) => assert!(saida.contains("ok html"), "com cota {n}: {saida}"),
            Err(e) => {
                assert_eq!(e, MSG_COTA, "com cota {n}");
                viu_erro = true;
            }
        }
    }
    assert!(viu_erro);
}

#[test]
fn linhas_em_branco_longas_colapsam_e_curtas_ficam() {
    let bytes = MsgBuilder::new().string(tags::BODY, "a\n\n\n\nb\n\nc").build();
    assert!(md(&bytes).contains("a\n\nb\n\nc"));
}

#[test]
fn data_vem_do_filetime_sem_cabecalho_de_transporte() {
    let bytes = MsgBuilder::new()
        .time(tags::CLIENT_SUBMIT_TIME, 134_370_775_200_000_000)
        .string(tags::BODY, "x")
        .build();
    assert!(md(&bytes).contains("- **Data:** 2026-10-21 17:32 UTC"));
}

#[test]
fn anexos_e_mensagem_embutida() {
    let embutida = MsgBuilder::new().string(tags::SUBJECT, "Original").string(tags::BODY, "oi");
    let bytes = MsgBuilder::new()
        .string(tags::BODY, "principal")
        .attachment(AttachSpec { name: "a.txt".into(), data: vec![0; 512], ..Default::default() })
        .attachment(AttachSpec { name: "msg.msg".into(), data: vec![], embedded: Some(embutida), ..Default::default() })
        .build();
    let saida = md(&bytes);
    assert!(saida.contains("- **Anexos:** a.txt (512 B)\n"));
    assert!(!saida.contains("msg.msg"));
    assert!(saida.contains("\n\n---\n\n> ## Mensagem anexada: Original\n>\n> oi\n"));
}

#[test]
fn smime_mostra_aviso() {
    let bytes = MsgBuilder::new()
        .string(tags::SUBJECT, "Assinado")
        .string(tags::MESSAGE_CLASS, "IPM.Note.SMIME.MultipartSigned")
        .string(tags::BODY, "segredo")
        .build();
    let saida = md(&bytes);
    assert!(saida.contains("S/MIME"));
    assert!(!saida.contains("segredo"));
}

#[test]
fn rtf_corrompido_cai_no_texto() {
    let bytes = MsgBuilder::new()
        .binary(tags::RTF_COMPRESSED, b"lixo lixo lixo lixo lixo lixo")
        .string(tags::BODY, "ok")
        .build();
    assert!(md(&bytes).contains("ok"));
}

#[test]
fn sem_corpo_algum() {
    let bytes = MsgBuilder::new().string(tags::SUBJECT, "Vazio").build();
    assert!(md(&bytes).contains("*(mensagem sem corpo)*"));
}

#[test]
fn sem_assunto_usa_marcador() {
    let bytes = MsgBuilder::new().string(tags::BODY, "x").build();
    assert!(md(&bytes).starts_with("# (sem assunto)\n"));
}

#[test]
fn charset_do_meta_decodifica_o_html() {
    let html = "<html><head><meta charset=\"utf-8\"></head><body><p>ação</p></body></html>";
    let bytes = MsgBuilder::new().binary(tags::HTML, html.as_bytes()).build();
    assert!(md(&bytes).contains("ação"));
}

#[test]
fn texto_que_passa_do_teto_de_saida_e_recusado() {
    // 17 MiB em ANSI (1 byte por caractere): pouco acima do teto de 16 MiB de saída, abaixo do de entrada.
    let corpo = vec![b'x'; 17 * 1024 * 1024];
    let bytes = MsgBuilder::new().string8(tags::BODY, &corpo).build();
    assert_eq!(to_markdown(&bytes, &Unlimited).unwrap_err(), "e-mail grande demais para converter");
}

#[test]
fn mensagem_grande_demais_e_recusada_sem_ler() {
    let grande = vec![0u8; crate::MAX_MSG_BYTES as usize + 1];
    assert_eq!(to_markdown(&grande, &Unlimited).unwrap_err(), "arquivo .msg grande demais");
}
