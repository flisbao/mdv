//! Texto de célula acumulado em eventos XML sem passar do teto, e a regra única de "texto seguro"
//! (sem controles, cortado em `max` caracteres + `…`).

/// Acumula pedaços de texto guardando no máximo `max + 1` caracteres; o caractere extra marca o
/// corte. Custa O(pedaço) por evento: a contagem de caracteres fica num campo.
///
/// O conteúdo é o que `mdv_text::sanitize` daria ao texto inteiro, caractere a caractere. A única
/// regra que atravessa caracteres é a do `\r`, copiada de `sanitize`: um `\r` no fim fica pendente
/// e vira `\n` se o próximo caractere (talvez em outro pedaço) for `\n`, senão vira U+FFFD. Se
/// `sanitize` mudar essa regra, o teste `junta_como_sanitize_no_texto_inteiro` falha.
#[derive(Debug)]
pub(crate) struct TextoLimitado {
    texto: String,
    chars: usize,
    max: usize,
}

impl TextoLimitado {
    pub fn novo(max: usize) -> Self {
        TextoLimitado { texto: String::new(), chars: 0, max }
    }

    pub fn acrescentar(&mut self, trecho: &str) {
        let limite = self.max + 1;
        for c in trecho.chars() {
            if self.chars >= limite {
                break;
            }
            if self.texto.ends_with('\r') {
                self.texto.pop();
                if c == '\n' {
                    self.texto.push('\n');
                    continue;
                }
                self.texto.push('\u{FFFD}');
            }
            if c == '\r' {
                self.texto.push('\r');
            } else {
                let mut buf = [0u8; 4];
                self.texto.push_str(&mdv_text::sanitize(c.encode_utf8(&mut buf)));
            }
            self.chars += 1;
        }
    }

    /// `true` se passou de `max` caracteres (o excedente não foi guardado).
    pub fn cortado(&self) -> bool {
        self.chars > self.max
    }

    /// O que foi guardado: no máximo `max + 1` caracteres, ainda sem o corte final. Quem usa o
    /// texto na saída passa por `texto_seguro`, que sanitiza o `\r` pendente e corta.
    pub fn into_bruto(self) -> String {
        self.texto
    }
}

/// Texto que entra na saída: sem caracteres de controle e cortado em `max` caracteres (+ `…`).
/// É o único ponto de sanitize e corte do texto vindo do arquivo (`planilha::ler`).
pub(crate) fn texto_seguro(bruto: &str, max: usize) -> String {
    cortar(&mdv_text::sanitize(bruto), max)
}

fn cortar(texto: &str, max: usize) -> String {
    match texto.char_indices().nth(max) {
        Some((fim, _)) => format!("{}…", &texto[..fim]),
        None => texto.to_string(),
    }
}
