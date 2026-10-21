//! Amostra `samples/email.msg`: e-mail totalmente fictício (Ana Souza, Bruno Lima, Carla Dias,
//! Diego Rocha, exemplo.com.br). O arquivo é gerado pelo `MsgBuilder`; `gerar_samples_email_msg`
//! o regrava (`-- --ignored`), e `sample_commitado_bate_com_o_gerador` garante que o commitado
//! converte para o mesmo Markdown que o gerador atual.
//! Rodar: `cargo test -p mdv-msg --features testutil --test sample`.

use mdv_msg::testutil::{tags, AttachSpec, MsgBuilder};
use mdv_msg::{to_markdown, Unlimited};
use std::fs;
use std::path::PathBuf;

const SUBJECT: &str = "Re: Fechamento de outubro";

const HTML: &str = r#"<html><head><meta charset="utf-8"></head><body>
<div>
<table role="presentation" width="100%" cellpadding="0" cellspacing="0" border="0"><tr><td>
<p>Olá, Bruno!</p>
<p><img src="cid:logo.png@01" alt="Logo"></p>
<p>Segue o fechamento de outubro. Os números estão na tabela abaixo; se houver algum ajuste, me avisa até sexta.</p>
<table border="1" cellpadding="4" cellspacing="0">
<tr><th>Área</th><th>Valor</th><th>Situação</th></tr>
<tr><td>Receita</td><td>R$ 152.300,00</td><td>Fechado</td></tr>
<tr><td>Despesas</td><td>R$ 98.750,40</td><td>Em revisão</td></tr>
<tr><td>Ação social</td><td>R$ 5.000,00</td><td>Aprovado</td></tr>
</table>
<p>Pontos de atenção:</p>
<ul>
<li>Conferir a nota do fornecedor de setembro</li>
<li>Atualizar a previsão de caixa de novembro</li>
</ul>
<p>Abraço,<br>Ana</p>
<hr style="display:inline-block;width:98%" tabindex="-1">
<div id="divRplyFwdMsg" dir="ltr">
<p><b>De:</b> Bruno Lima &lt;bruno.lima@exemplo.com.br&gt;<br>
<b>Enviado:</b> terça-feira, 20 de outubro de 2026 17:05<br>
<b>Para:</b> Ana Souza &lt;ana.souza@exemplo.com.br&gt;<br>
<b>Assunto:</b> Fechamento de outubro</p>
</div>
<div>
<p>Oi, Ana. Você pode revisar os números de outubro até sexta?</p>
<p>Obrigado,<br>Bruno</p>
</div>
</td></tr></table>
</div>
</body></html>"#;

/// Monta o e-mail fictício da amostra.
fn monta_email() -> MsgBuilder {
    let planilha: Vec<u8> = (0..2048).map(|i| (i * 7 % 251) as u8).collect();
    MsgBuilder::new()
        .string(tags::SUBJECT, SUBJECT)
        .string(tags::MESSAGE_CLASS, "IPM.Note")
        .string(tags::SENDER_NAME, "Ana Souza")
        .string(tags::SENDER_SMTP_ADDRESS, "ana.souza@exemplo.com.br")
        .string(
            tags::TRANSPORT_MESSAGE_HEADERS,
            "Date: Wed, 21 Oct 2026 14:32:10 -0300\r\nFrom: Ana Souza <ana.souza@exemplo.com.br>\r\n",
        )
        .recipient(1, "Bruno Lima", "bruno.lima@exemplo.com.br")
        .recipient(1, "Carla Dias", "carla.dias@exemplo.com.br")
        .recipient(2, "Diego Rocha", "diego.rocha@exemplo.com.br")
        .binary(tags::HTML, HTML.as_bytes())
        .attachment(AttachSpec {
            name: "planilha.xlsx".to_string(),
            data: planilha,
            mime: Some("application/vnd.openxmlformats-officedocument.spreadsheetml.sheet".to_string()),
            ..AttachSpec::default()
        })
        .attachment(AttachSpec {
            name: "logo.png".to_string(),
            data: include_bytes!("../../../samples/logo.png").to_vec(),
            mime: Some("image/png".to_string()),
            hidden: true,
            ..AttachSpec::default()
        })
}

fn caminho_amostra() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../samples/email.msg")
}

#[test]
#[ignore = "regrava samples/email.msg; rodar com --ignored"]
fn gerar_samples_email_msg() {
    fs::write(caminho_amostra(), monta_email().build()).expect("gravar samples/email.msg");
}

#[test]
fn sample_commitado_bate_com_o_gerador() {
    let commitado = fs::read(caminho_amostra()).expect("samples/email.msg deve existir");
    let do_arquivo = to_markdown(&commitado, &Unlimited).expect("amostra converte");
    let do_gerador = to_markdown(&monta_email().build(), &Unlimited).expect("gerador converte");
    assert_eq!(do_arquivo, do_gerador);
    assert!(do_arquivo.contains(SUBJECT), "{do_arquivo}");
}

#[test]
fn amostra_convertida_mostra_imagem_tabela_e_citacao() {
    let commitado = fs::read(caminho_amostra()).expect("samples/email.msg deve existir");
    let md = to_markdown(&commitado, &Unlimited).expect("amostra converte");
    assert!(md.contains("[imagem: Logo]"), "{md}");
    assert!(md.contains("| Área | Valor | Situação |"), "{md}");
    assert!(!md.contains("logo.png ("), "imagem oculta não entra em Anexos: {md}");
    assert!(!md.contains("> ---"), "o <hr> antes da resposta some: {md}");
    assert!(md.contains("> **De:** Bruno Lima"), "{md}");
}
