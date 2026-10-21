//! Extração do corpo RTF de `.msg` (MS-OXRTFEX): HTML encapsulado (`\fromhtml1`) ou texto.
//! O tokenizador não usa recursão: grupos ficam numa pilha, com teto de profundidade.

use crate::codepage;
use crate::{Budget, MAX_RTF_BYTES};

/// Profundidade máxima de grupos aninhados aceita.
const MAX_PROFUNDIDADE: usize = 1000;
/// Intervalo, em bytes de entrada ou de saída, entre consultas ao orçamento.
const INTERVALO_CHECK: usize = 64 * 1024;
/// Destinos cujo conteúdo nunca vai para a saída.
const DESTINOS_IGNORADOS: &[&str] = &[
    "fonttbl", "colortbl", "stylesheet", "info", "pict", "object", "header", "footer",
];
/// Limite do valor de `\ucN`, para não pular a entrada inteira por engano.
const UC_MAXIMO: i64 = 1 << 20;

/// Corpo extraído de um RTF.
#[derive(Debug, PartialEq)]
pub(crate) enum RtfBody {
    Html(String),
    Text(String),
}

/// Situação do primeiro token de um grupo, para detectar destinos (`{\*\...}`).
#[derive(Clone, Copy, PartialEq)]
enum Destino {
    Nenhum,
    Inicio,
    Estrela,
}

/// Estado herdado por grupo; é restaurado ao fechar o grupo.
#[derive(Clone, Copy)]
struct Estado {
    ignorar: bool,
    htmltag: bool,
    html_rtf: bool,
    uc: usize,
    destino: Destino,
}

impl Estado {
    fn inicial() -> Self {
        Estado { ignorar: false, htmltag: false, html_rtf: false, uc: 1, destino: Destino::Nenhum }
    }
}

struct Extrator<'a> {
    budget: &'a dyn Budget,
    cur: Estado,
    pilha: Vec<Estado>,
    html: bool,
    cp: u32,
    buf: Vec<u8>,
    out: String,
    saltar: usize,
    alto: Option<u32>,
    prox_check_saida: usize,
}

/// Extrai o corpo de um RTF já descomprimido. Modo HTML se o documento traz `\fromhtml1`.
pub(crate) fn extract(rtf: &[u8], budget: &dyn Budget) -> Result<RtfBody, String> {
    if rtf.len() > MAX_RTF_BYTES {
        return Err("corpo RTF grande demais".to_string());
    }
    let mut ex = Extrator {
        budget,
        cur: Estado::inicial(),
        pilha: Vec::new(),
        html: false,
        cp: 1252,
        buf: Vec::new(),
        out: String::new(),
        saltar: 0,
        alto: None,
        prox_check_saida: INTERVALO_CHECK,
    };
    ex.varre(rtf)?;
    ex.fronteira()?;
    Ok(if ex.html { RtfBody::Html(ex.out) } else { RtfBody::Text(ex.out) })
}

impl Extrator<'_> {
    fn varre(&mut self, rtf: &[u8]) -> Result<(), String> {
        let mut i = 0;
        let mut prox_check_entrada = INTERVALO_CHECK;
        while i < rtf.len() {
            if i >= prox_check_entrada {
                self.budget.check()?;
                prox_check_entrada = i + INTERVALO_CHECK;
            }
            match rtf[i] {
                b'{' => {
                    self.fecha_destino();
                    self.fronteira()?;
                    if self.pilha.len() >= MAX_PROFUNDIDADE {
                        return Err("corpo RTF corrompido (aninhamento)".to_string());
                    }
                    self.pilha.push(self.cur);
                    self.cur.destino = Destino::Inicio;
                    i += 1;
                }
                b'}' => {
                    self.fecha_destino();
                    self.fronteira()?;
                    if let Some(pai) = self.pilha.pop() {
                        self.cur = pai;
                    }
                    i += 1;
                }
                b'\\' => i = self.controle(rtf, i)?,
                b'\r' | b'\n' => i += 1,
                c => {
                    self.byte(c)?;
                    i += 1;
                }
            }
        }
        Ok(())
    }

    /// Trata `\` na posição `i` e devolve a posição seguinte.
    fn controle(&mut self, rtf: &[u8], i: usize) -> Result<usize, String> {
        let n = rtf.len();
        let j = i + 1;
        if j >= n {
            return Ok(n);
        }
        let c = rtf[j];
        if c.is_ascii_alphabetic() {
            let mut k = j;
            while k < n && rtf[k].is_ascii_alphabetic() {
                k += 1;
            }
            let nome = std::str::from_utf8(&rtf[j..k]).unwrap_or("");
            let neg = k + 1 < n && rtf[k] == b'-' && rtf[k + 1].is_ascii_digit();
            let mut q = if neg { k + 1 } else { k };
            let mut param = None;
            if q < n && rtf[q].is_ascii_digit() {
                let inicio = q;
                while q < n && rtf[q].is_ascii_digit() {
                    q += 1;
                }
                let v = rtf[inicio..q]
                    .iter()
                    .fold(0i64, |acc, d| acc.saturating_mul(10).saturating_add(i64::from(d - b'0')));
                param = Some(if neg { -v } else { v });
                k = q;
            }
            if k < n && rtf[k] == b' ' {
                k += 1;
            }
            self.palavra(nome, param)?;
            return Ok(k);
        }
        if c == b'\'' {
            if let (Some(h), Some(l)) = (hex(rtf.get(j + 1)), hex(rtf.get(j + 2))) {
                self.byte(h * 16 + l)?;
                return Ok(j + 3);
            }
            return Ok(j + 1);
        }
        match c {
            b'*' => {
                if self.cur.destino == Destino::Inicio {
                    self.cur.destino = Destino::Estrela;
                }
            }
            b'\\' | b'{' | b'}' => self.byte(c)?,
            b'~' => self.byte(b' ')?,
            _ => {}
        }
        Ok(j + 1)
    }

    /// Trata uma palavra de controle; `nome` vem sem a barra e `param` é o número, se houver.
    fn palavra(&mut self, nome: &str, param: Option<i64>) -> Result<(), String> {
        match self.cur.destino {
            Destino::Estrela => {
                if nome == "htmltag" {
                    self.cur.htmltag = true;
                } else {
                    self.cur.ignorar = true;
                }
                self.cur.destino = Destino::Nenhum;
                return Ok(());
            }
            Destino::Inicio => {
                if DESTINOS_IGNORADOS.contains(&nome) {
                    self.cur.ignorar = true;
                }
                self.cur.destino = Destino::Nenhum;
            }
            Destino::Nenhum => {}
        }
        if nome == "u" {
            return self.unicode(param);
        }
        self.fronteira()?;
        let perm = self.permitido();
        match nome {
            "ansicpg" => {
                if let Some(cp) = param.and_then(|n| u32::try_from(n).ok()) {
                    self.cp = cp;
                }
            }
            "fromhtml" => {
                if param != Some(0) {
                    self.html = true;
                }
            }
            "htmlrtf" => self.cur.html_rtf = param != Some(0),
            "uc" => {
                let n = param.unwrap_or(1).clamp(0, UC_MAXIMO);
                self.cur.uc = usize::try_from(n).unwrap_or(1);
            }
            "par" if perm => {
                let quebra = if self.html { "\r\n" } else { "\n" };
                self.emite(quebra)?;
            }
            "line" if perm => self.emite("\n")?,
            "tab" if perm => self.emite("\t")?,
            "cell" if perm && !self.html => self.emite("\t")?,
            "row" if perm && !self.html => self.emite("\n")?,
            _ => {}
        }
        Ok(())
    }

    /// Trata `\uN`: emite o caractere e pula os `uc` caracteres seguintes.
    fn unicode(&mut self, param: Option<i64>) -> Result<(), String> {
        let Some(n) = param else {
            return Ok(());
        };
        self.saltar = self.cur.uc;
        if !self.permitido() {
            return Ok(());
        }
        let n = n.clamp(-65536, 65535);
        let cp = if n < 0 { n + 65536 } else { n } as u32;
        match cp {
            0xD800..=0xDBFF => {
                self.resolve_alto()?;
                self.alto = Some(cp);
            }
            0xDC00..=0xDFFF => match self.alto.take() {
                Some(alto) => {
                    let c = 0x10000 + ((alto - 0xD800) << 10) + (cp - 0xDC00);
                    let ch = char::from_u32(c).unwrap_or('\u{FFFD}');
                    self.emite(ch.encode_utf8(&mut [0; 4]))?;
                }
                None => self.emite("\u{FFFD}")?,
            },
            _ => {
                let ch = char::from_u32(cp).unwrap_or('\u{FFFD}');
                self.emite(ch.encode_utf8(&mut [0; 4]))?;
            }
        }
        Ok(())
    }

    /// Trata um byte de texto ou de `\'hh`: pula se for caractere de `\uc`, senão acumula.
    fn byte(&mut self, b: u8) -> Result<(), String> {
        self.fecha_destino();
        if self.saltar > 0 {
            self.saltar -= 1;
            return Ok(());
        }
        if self.permitido() {
            self.resolve_alto()?;
            self.buf.push(b);
        }
        Ok(())
    }

    /// Um destino `{\*\...}` que não é `htmltag` vira grupo ignorado.
    fn fecha_destino(&mut self) {
        if self.cur.destino == Destino::Estrela {
            self.cur.ignorar = true;
        }
        self.cur.destino = Destino::Nenhum;
    }

    /// Diz se o conteúdo do grupo atual deve ir para a saída.
    fn permitido(&self) -> bool {
        if self.cur.ignorar {
            return false;
        }
        if self.cur.htmltag {
            return self.html;
        }
        if self.html {
            return !self.cur.html_rtf;
        }
        true
    }

    /// Descarrega bytes pendentes e surrogate alto sem par, antes de qualquer fronteira.
    fn fronteira(&mut self) -> Result<(), String> {
        self.resolve_alto()?;
        self.descarrega()
    }

    /// Surrogate alto que não ganhou par vira U+FFFD.
    fn resolve_alto(&mut self) -> Result<(), String> {
        if self.alto.take().is_some() {
            self.descarrega()?;
            self.saida("\u{FFFD}")?;
        }
        Ok(())
    }

    /// Texto emitido diretamente (quebras, tabulações, caracteres unicode).
    fn emite(&mut self, s: &str) -> Result<(), String> {
        self.fronteira()?;
        self.saida(s)
    }

    /// Decodifica os bytes acumulados pela codepage atual e os envia à saída.
    fn descarrega(&mut self) -> Result<(), String> {
        if self.buf.is_empty() {
            return Ok(());
        }
        let texto = codepage::decode(&self.buf, self.cp);
        self.buf.clear();
        self.saida(&texto)
    }

    fn saida(&mut self, s: &str) -> Result<(), String> {
        self.out.push_str(s);
        if self.out.len() > MAX_RTF_BYTES {
            return Err("corpo RTF grande demais".to_string());
        }
        if self.out.len() >= self.prox_check_saida {
            self.budget.check()?;
            self.prox_check_saida = self.out.len() + INTERVALO_CHECK;
        }
        Ok(())
    }
}

/// Valor de um dígito hexadecimal opcional.
fn hex(b: Option<&u8>) -> Option<u8> {
    b.and_then(|&c| (c as char).to_digit(16)).map(|d| d as u8)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Unlimited;

    fn rtf_html(rtf: &[u8]) -> String {
        match extract(rtf, &Unlimited).unwrap() {
            RtfBody::Html(h) => h,
            outro => panic!("esperava HTML, veio {outro:?}"),
        }
    }

    #[test]
    fn html_encapsulado_e_extraido() {
        let rtf = br"{\rtf1\ansi\ansicpg1252\fromhtml1 \deff0{\fonttbl{\f0 Arial;}}{\*\htmltag19 <html>}{\*\htmltag50 <body>}\htmlrtf {\f0 lixo}\htmlrtf0 Ol\'e1 mundo{\*\htmltag58 </body>}{\*\htmltag27 </html>}}";
        assert_eq!(rtf_html(rtf), "<html><body>Olá mundo</body></html>");
    }

    #[test]
    fn html_encapsulado_decodifica_hex_pela_ansicpg() {
        let rtf = br"{\rtf1\ansi\ansicpg1252\fromhtml1 {\*\htmltag64 <p>}a\'e7\'e3o{\*\htmltag72 </p>}}";
        assert_eq!(rtf_html(rtf), "<p>ação</p>");
    }

    #[test]
    fn unicode_pula_o_caractere_substituto() {
        let rtf = br"{\rtf1\ansi\fromhtml1 {\*\htmltag64 <p>}\uc1\u8364?{\*\htmltag72 </p>}}";
        assert_eq!(rtf_html(rtf), "<p>€</p>");
    }

    #[test]
    fn par_fora_de_htmlrtf_vira_quebra() {
        assert_eq!(rtf_html(br"{\rtf1\fromhtml1 a\par b}"), "a\r\nb");
    }

    #[test]
    fn rtf_sem_fromhtml_vira_texto() {
        let rtf = br"{\rtf1\ansi{\fonttbl{\f0 Arial;}}{\colortbl;\red0\green0\blue0;}Linha 1\par Linha\tab 2}";
        assert!(matches!(extract(rtf, &Unlimited).unwrap(), RtfBody::Text(t) if t == "Linha 1\nLinha\t2"));
    }

    #[test]
    fn chaves_desbalanceadas_nao_entram_em_panico() {
        let _ = extract(br"{\rtf1 {{{a", &Unlimited);
        let _ = extract(b"}}}", &Unlimited);
    }

    #[test]
    fn aninhamento_absurdo_e_recusado() {
        let entrada = "{".repeat(2000);
        let erro = extract(entrada.as_bytes(), &Unlimited).unwrap_err();
        assert!(erro.contains("aninhamento"), "erro: {erro}");
    }
}
