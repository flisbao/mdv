//! Montagem do Markdown de uma mensagem: cabeçalho, corpo (HTML, RTF ou texto) e mensagens embutidas.

use crate::codepage;
use crate::date;
use crate::escape;
use crate::html;
use crate::lzfu;
use crate::message::{Attachment, Message, Recipient};
use crate::rtf::{self, RtfBody};
use crate::{Budget, MAX_MARKDOWN_BYTES, MAX_RTF_BYTES};

const CP_UTF8: u32 = 65001;
/// Quantos bytes do HTML são examinados atrás de `charset=`.
const JANELA_CHARSET: usize = 4096;
const AVISO_SMIME: &str = "> [!WARNING]\n> Mensagem assinada ou cifrada (S/MIME): o mdv não decifra nem verifica o corpo.";
const SEM_CORPO: &str = "*(mensagem sem corpo)*";
const EXCESSO: &str = "e-mail grande demais para converter";

/// Markdown de uma mensagem. `depth` 0 é a raiz; embutidas recebem `depth + 1`.
/// Termina com exatamente um `\n`.
pub(crate) fn render(m: &Message, depth: usize, budget: &dyn Budget) -> Result<String, String> {
    budget.check()?;
    let mut saida = format!("{} {}", "#".repeat((depth + 1).min(6)), titulo(m, depth));
    let cabecalho = cabecalho_linhas(m);
    if !cabecalho.is_empty() {
        saida.push_str("\n\n");
        saida.push_str(&cabecalho.join("\n"));
        saida.push_str("\n\n---");
    }
    saida.push_str("\n\n");
    saida.push_str(&corpo(m, budget)?);
    if saida.len() > MAX_MARKDOWN_BYTES {
        return Err(EXCESSO.to_string());
    }
    for anexo in m.attachments.iter().filter(|a| a.embedded.is_some()) {
        budget.check()?;
        let Some(embutida) = anexo.embedded.as_deref() else { continue };
        let md = render(embutida, depth + 1, budget)?;
        saida.push_str("\n\n---\n\n");
        saida.push_str(&citar(md.trim_end_matches('\n')));
        if saida.len() > MAX_MARKDOWN_BYTES {
            return Err(EXCESSO.to_string());
        }
    }
    let mut resultado = saida.trim_end_matches('\n').to_string();
    resultado.push('\n');
    Ok(resultado)
}

/// Título da mensagem: assunto na raiz, `Mensagem anexada: <assunto>` nas embutidas.
fn titulo(m: &Message, depth: usize) -> String {
    let assunto = m
        .subject
        .as_deref()
        .map(linha_unica)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "(sem assunto)".to_string());
    let mut texto = if depth == 0 { assunto } else { format!("Mensagem anexada: {assunto}") };
    // Um `#` final seria lido como fechamento de título ATX: escapa-se.
    if texto.ends_with('#') {
        texto.pop();
        texto.push_str("\\#");
    }
    texto
}

/// Linhas `- **Campo:** valor` do cabeçalho; campos ausentes somem.
fn cabecalho_linhas(m: &Message) -> Vec<String> {
    let transporte = m.transport_headers.as_deref().unwrap_or("");
    let mut linhas = Vec::new();
    let de = remetente(m, transporte);
    if let Some(de) = de {
        linhas.push(format!("- **De:** {de}"));
    }
    if let Some(para) = destinatarios(&m.to, m.display_to.as_deref(), m.recipients_truncated) {
        linhas.push(format!("- **Para:** {para}"));
    }
    if let Some(cc) = destinatarios(&m.cc, m.display_cc.as_deref(), m.recipients_truncated) {
        linhas.push(format!("- **Cc:** {cc}"));
    }
    if let Some(data) = data(m, transporte) {
        linhas.push(format!("- **Data:** {data}"));
    }
    if let Some(anexos) = anexos(&m.attachments, m.attachments_truncated) {
        linhas.push(format!("- **Anexos:** {anexos}"));
    }
    linhas
}

fn remetente(m: &Message, transporte: &str) -> Option<String> {
    pessoa(m.sender_name.as_deref(), m.sender_email.as_deref())
        .or_else(|| date::header(transporte, "From").map(|v| linha_unica(&v)).filter(|v| !v.is_empty()))
}

/// Lista de destinatários; sem nomes legíveis, usa o texto de exibição do Outlook.
fn destinatarios(lista: &[Recipient], exibicao: Option<&str>, truncado: bool) -> Option<String> {
    let nomes: Vec<String> = lista.iter().filter_map(|r| pessoa(r.name.as_deref(), r.email.as_deref())).collect();
    let texto = if nomes.is_empty() {
        exibicao.map(linha_unica).filter(|t| !t.is_empty())?
    } else {
        nomes.join("; ")
    };
    Some(if truncado { format!("{texto} (e outros)") } else { texto })
}

/// `Nome <email>`, só o nome ou só o e-mail; os dois campos são escapados separadamente.
fn pessoa(nome: Option<&str>, email: Option<&str>) -> Option<String> {
    let nome = nome.map(linha_unica).filter(|s| !s.is_empty());
    let email = email.map(linha_unica).filter(|s| !s.is_empty());
    match (nome, email) {
        (Some(n), Some(e)) => Some(format!("{n} \\<{e}\\>")),
        (Some(n), None) => Some(n),
        (None, Some(e)) => Some(e),
        (None, None) => None,
    }
}

/// Data do cabeçalho de transporte, senão a de envio e a de entrega (FILETIME), em UTC.
fn data(m: &Message, transporte: &str) -> Option<String> {
    date::header(transporte, "Date")
        .and_then(|v| date::from_rfc2822(&v))
        .or_else(|| m.submit_time.filter(|&t| t != 0).map(date::from_filetime))
        .or_else(|| m.delivery_time.filter(|&t| t != 0).map(date::from_filetime))
}

/// Anexos visíveis (sem os ocultos e sem as mensagens embutidas, que têm seção própria).
fn anexos(lista: &[Attachment], truncado: bool) -> Option<String> {
    let nomes: Vec<String> = lista
        .iter()
        .filter(|a| !a.hidden && a.embedded.is_none())
        .map(descreve_anexo)
        .collect();
    if nomes.is_empty() {
        return None;
    }
    let texto = nomes.join("; ");
    Some(if truncado { format!("{texto} (e outros)") } else { texto })
}

fn descreve_anexo(a: &Attachment) -> String {
    let nome = linha_unica(&a.name);
    let mut detalhes = Vec::new();
    if let Some(tamanho) = a.size {
        detalhes.push(tamanho_legivel(tamanho));
    }
    if let Some(mime) = a.mime.as_deref().map(linha_unica).filter(|s| !s.is_empty()) {
        detalhes.push(mime);
    }
    if detalhes.is_empty() {
        nome
    } else {
        format!("{nome} ({})", detalhes.join(", "))
    }
}

/// `N B` abaixo de 1 KiB, `N KiB` (arredondado) abaixo de 1 MiB, senão `N,N MiB`.
fn tamanho_legivel(n: u32) -> String {
    const KIB: u64 = 1024;
    const MIB: u64 = 1024 * 1024;
    let n = u64::from(n);
    if n < KIB {
        format!("{n} B")
    } else if n < MIB {
        format!("{} KiB", (n + KIB / 2) / KIB)
    } else {
        let decimos = (n * 10 + MIB / 2) / MIB;
        format!("{},{} MiB", decimos / 10, decimos % 10)
    }
}

/// Corpo da mensagem: S/MIME avisa; depois HTML, RTF (HTML ou texto) e texto puro, nesta ordem.
/// Erro de orçamento (`Budget`) aborta; erro de um passo só o tira da lista de tentativas.
fn corpo(m: &Message, budget: &dyn Budget) -> Result<String, String> {
    if m.class.as_deref().is_some_and(|c| c.to_ascii_lowercase().starts_with("ipm.note.smime")) {
        return Ok(AVISO_SMIME.to_string());
    }
    let mut erros = Vec::new();
    if let Some(html) = &m.html {
        budget.check()?;
        if let Some(md) = aceita(html_para_md(html, m.html_codepage, budget), &mut erros, budget)? {
            return Ok(md);
        }
    }
    budget.check()?;
    let rtf = match &m.rtf {
        Some(bruto) => match rtf_corpo(bruto, budget) {
            Ok(corpo) => Some(corpo),
            Err(e) => {
                budget.check()?;
                erros.push(e);
                None
            }
        },
        None => None,
    };
    if let Some(RtfBody::Html(h)) = &rtf {
        budget.check()?;
        if let Some(md) = aceita(html::to_markdown(h, budget), &mut erros, budget)? {
            return Ok(md);
        }
    }
    if let Some(texto) = m.text.as_deref().filter(|t| !t.trim().is_empty()) {
        return texto_puro(texto, budget);
    }
    if let Some(RtfBody::Text(texto)) = &rtf {
        if !texto.trim().is_empty() {
            return texto_puro(texto, budget);
        }
    }
    budget.check()?;
    if erros.is_empty() {
        Ok(SEM_CORPO.to_string())
    } else {
        Ok(format!("> [!WARNING]\n> Não consegui ler o corpo: {}", erros.join("; ")))
    }
}

/// Um passo do corpo só vale se produziu texto. Regra de orçamento: só a falha de `Budget::check`
/// (memória viva acima do limite) aborta; a recusa de `check_html` (HTML grande demais, nada foi
/// alocado) é um erro do passo, e o corpo cai no próximo passo (RTF ou texto puro).
/// Por isso o erro do passo é seguido de `check()?`.
fn aceita(
    resultado: Result<String, String>,
    erros: &mut Vec<String>,
    budget: &dyn Budget,
) -> Result<Option<String>, String> {
    match resultado {
        Ok(md) if !md.trim().is_empty() => Ok(Some(md)),
        Ok(_) => Ok(None),
        Err(e) => {
            budget.check()?;
            erros.push(e);
            Ok(None)
        }
    }
}

fn html_para_md(bytes: &[u8], cp_html: u32, budget: &dyn Budget) -> Result<String, String> {
    html::to_markdown(&decodifica_html(bytes, cp_html), budget)
}

/// Decodifica o HTML: string UTF-8 (cp 65001) não é reinterpretada; senão vale o `<meta charset>`
/// dos primeiros bytes, e na falta dele, a codepage da mensagem.
fn decodifica_html(bytes: &[u8], cp_html: u32) -> String {
    if cp_html == CP_UTF8 {
        return codepage::decode(bytes, CP_UTF8);
    }
    charset_do_meta(bytes)
        .and_then(|rotulo| codepage::decode_label(bytes, &rotulo))
        .unwrap_or_else(|| codepage::decode(bytes, cp_html))
}

/// Primeiro `charset=` (sem diferenciar caixa) nos primeiros 4 KiB que seja um rótulo conhecido.
fn charset_do_meta(bytes: &[u8]) -> Option<String> {
    let inicio = &bytes[..bytes.len().min(JANELA_CHARSET)];
    let minusc = inicio.to_ascii_lowercase();
    const MARCA: &[u8] = b"charset=";
    let mut pos = 0;
    while let Some(i) = minusc[pos..].windows(MARCA.len()).position(|w| w == MARCA) {
        let ini = pos + i + MARCA.len();
        let resto = &minusc[ini..];
        let resto = match resto.first() {
            Some(b'"' | b'\'') => &resto[1..],
            _ => resto,
        };
        let rotulo: Vec<u8> = resto
            .iter()
            .take_while(|b| !matches!(b, b'"' | b'\'' | b';' | b' ' | b'>' | b'/' | b'\t' | b'\r' | b'\n'))
            .copied()
            .collect();
        if let Ok(rotulo) = String::from_utf8(rotulo) {
            if encoding_rs::Encoding::for_label(rotulo.as_bytes()).is_some() {
                return Some(rotulo);
            }
        }
        pos = ini;
    }
    None
}

fn rtf_corpo(bruto: &[u8], budget: &dyn Budget) -> Result<RtfBody, String> {
    let rtf = lzfu::decompress(bruto, MAX_RTF_BYTES)?;
    rtf::extract(&rtf, budget)
}

/// Texto puro: quebras normalizadas. Linhas consecutivas viram quebra forçada (`\` no fim da linha),
/// porque o mdv renderiza quebra simples como espaço. Recuo vira espaços não separáveis (tab = 4),
/// para não virar bloco de código. Blocos separados por linhas em branco; três ou mais viram uma.
fn texto_puro(texto: &str, budget: &dyn Budget) -> Result<String, String> {
    let mut linhas: Vec<String> = Vec::new();
    let mut vazias = 0usize;
    let mut tamanho = 0usize;
    for linha in texto.replace("\r\n", "\n").replace('\r', "\n").split('\n') {
        budget.check()?;
        if linha.trim().is_empty() {
            vazias += 1;
            continue;
        }
        if let Some(anterior) = linhas.last_mut().filter(|_| vazias == 0) {
            anterior.push('\\');
        } else if !linhas.is_empty() {
            let brancos = if vazias >= 3 { 1 } else { vazias };
            for _ in 0..brancos {
                linhas.push(String::new());
            }
        }
        vazias = 0;
        let sem_recuo = linha.trim_start_matches([' ', '\t']);
        let recuo = &linha[..linha.len() - sem_recuo.len()];
        let largura: usize = recuo.chars().map(|c| if c == '\t' { 4 } else { 1 }).sum();
        let mut com_recuo = "\u{a0}".repeat(largura);
        com_recuo.push_str(sem_recuo);
        let escapada = escape::line_start(&mdv_text::sanitize(&com_recuo));
        tamanho += escapada.len() + 2;
        if tamanho > MAX_MARKDOWN_BYTES {
            return Err(EXCESSO.to_string());
        }
        linhas.push(escapada);
    }
    Ok(linhas.join("\n"))
}

/// Cita um bloco Markdown: cada linha ganha `> ` (linha vazia vira `>`).
fn citar(md: &str) -> String {
    md.split('\n')
        .map(|l| if l.is_empty() { ">".to_string() } else { format!("> {l}") })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Texto de uma linha só: controles viram U+FFFD, quebras e tabs viram espaço, depois escape inline.
fn linha_unica(s: &str) -> String {
    let plano: String = mdv_text::sanitize(s)
        .chars()
        .map(|c| if matches!(c, '\r' | '\n' | '\t') { ' ' } else { c })
        .collect();
    escape::inline(plano.trim())
}

#[cfg(test)]
#[path = "markdown_tests.rs"]
mod tests;
