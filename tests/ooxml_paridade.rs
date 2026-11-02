//! Paridade de segurança entre os leitores `.docx` e `.xlsx`. O ZIP e o XML são duplicados nas duas
//! crates (decisão do dono); este teste roda a mesma tabela de pacotes hostis nos dois
//! `to_markdown` e exige a mesma coisa: `Err`, sem caractere de controle na mensagem.
//! Se uma correção de segurança chegar a só uma das cópias, algum caso daqui falha.

use mdv_docx::testutil::zip;

const NS_REL: &str = "http://schemas.openxmlformats.org/package/2006/relationships";
const TIPO_PRINCIPAL: &str = "http://x/officeDocument";

/// Posição da última ocorrência de `assinatura` em `bytes`.
fn ultima(bytes: &[u8], assinatura: &[u8]) -> usize {
    bytes.windows(assinatura.len()).rposition(|w| w == assinatura).expect("assinatura no ZIP")
}

/// ZIP de uma entrada só, `_rels/.rels`, com `conteudo`.
fn com_rels(conteudo: &str) -> Vec<u8> {
    zip(&[("_rels/.rels", conteudo.as_bytes())], false)
}

fn aninhado(n: usize) -> String {
    format!("{}{}", "<a>".repeat(n), "</a>".repeat(n))
}

fn atributos(n: usize) -> String {
    let atributos: String = (0..n).map(|i| format!(" a{i}=\"x\"")).collect();
    format!("<Relationships{atributos}/>")
}

fn relacoes(n: usize) -> String {
    let itens: String = (0..n).map(|i| format!("<Relationship Id=\"r{i}\" Type=\"t\" Target=\"a\"/>")).collect();
    format!("<Relationships xmlns=\"{NS_REL}\">{itens}</Relationships>")
}

/// Pacotes hostis, com o nome do caso para a mensagem de falha.
fn casos() -> Vec<(&'static str, Vec<u8>)> {
    let mut tamanho = com_rels("<Relationships/>");
    let central = ultima(&tamanho, b"PK\x01\x02");
    tamanho[central + 24..central + 28].copy_from_slice(&5u32.to_le_bytes());

    let mut zip64 = com_rels("<Relationships/>");
    let eocd = ultima(&zip64, b"PK\x05\x06");
    zip64[eocd + 10..eocd + 12].copy_from_slice(&0xFFFFu16.to_le_bytes());

    let mut senha = com_rels("<Relationships/>");
    let central = ultima(&senha, b"PK\x01\x02");
    senha[central + 8] |= 1;

    let alvo = format!(
        "<Relationships xmlns=\"{NS_REL}\"><Relationship Id=\"r1\" Type=\"{TIPO_PRINCIPAL}\" Target=\"p/\u{1b}]0;X\u{7}.xml\"/></Relationships>"
    );
    let mut ole2 = vec![0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];
    ole2.resize(100, 0);

    vec![
        ("entrada duplicada com ESC", zip(&[("a\u{1b}]0;X\u{7}", b"1"), ("a\u{1b}]0;X\u{7}", b"2")], false)),
        ("tamanho declarado diferente do real", tamanho),
        ("ZIP64", zip64),
        ("bit de senha", senha),
        ("OLE2 (formato antigo ou criptografado)", ole2),
        ("não é ZIP", b"nao sou zip".to_vec()),
        ("alvo da parte principal com ESC e ausente", com_rels(&alvo)),
        ("DTD", com_rels("<!DOCTYPE x [<!ENTITY a \"b\">]><Relationships>&a;</Relationships>")),
        ("profundidade 101", com_rels(&aninhado(101))),
        ("257 atributos", com_rels(&atributos(257))),
        ("20 001 relações", com_rels(&relacoes(20_001))),
        ("tag de fechamento com ESC", com_rels("<a></b\u{1b}c>")),
    ]
}

fn confere(formato: &str, caso: &str, resultado: Result<String, String>) {
    let erro = resultado.expect_err(&format!("{formato}: {caso} deveria ser recusado"));
    assert!(!erro.is_empty(), "{formato}: {caso}: mensagem vazia");
    assert!(!erro.chars().any(char::is_control), "{formato}: {caso}: controle na mensagem: {erro:?}");
}

#[test]
fn pacotes_hostis_dao_erro_sem_controles_no_docx_e_no_xlsx() {
    for (caso, bytes) in casos() {
        confere("docx", caso, mdv_docx::to_markdown(&bytes, &mdv_docx::Unlimited));
        confere("xlsx", caso, mdv_xlsx::to_markdown(&bytes, &mdv_xlsx::Unlimited));
    }
}
