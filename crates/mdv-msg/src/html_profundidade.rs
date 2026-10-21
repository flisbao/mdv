//! Pré-varredura linear do HTML, antes do `html5ever`: estima quantos elementos ficariam abertos ao
//! mesmo tempo. O parser cresce de forma quadrática com o aninhamento; esta conta recusa a entrada cedo.
//! Não é um parser: segue só o que muda a pilha de elementos (comentários, texto cru, valores de
//! atributo entre aspas, elementos vazios e fechamentos sem abertura correspondente, que o parser ignora).
//! `/>` não fecha elemento HTML (o parser também não fecha), então não é tratado como vazio.

use crate::MAX_HTML_PARSE_DEPTH;
use std::collections::HashMap;

/// Elementos vazios: nunca ficam abertos.
const VAZIAS: &[&str] = &[
    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source", "track", "wbr",
];
/// Elementos cujo conteúdo é texto cru: um `<` dentro deles não abre tag.
const CRUS: &[&str] = &["iframe", "noembed", "noframes", "script", "style", "textarea", "title", "xmp"];
pub(crate) const MSG_ANINHADO: &str = "HTML aninhado demais";

/// Recusa o HTML cujo aninhamento estimado passa de `MAX_HTML_PARSE_DEPTH` elementos abertos.
pub(crate) fn confere(html: &str) -> Result<(), String> {
    let b = html.as_bytes();
    let mut pilha: Vec<Vec<u8>> = Vec::new();
    let mut abertos: HashMap<Vec<u8>, usize> = HashMap::new();
    let mut i = 0;
    while let Some(off) = b[i..].iter().position(|&c| c == b'<') {
        let at = i + off;
        if b[at..].starts_with(b"<!--") {
            i = if b[at + 4..].starts_with(b">") {
                at + 5
            } else if b[at + 4..].starts_with(b"->") {
                at + 6
            } else {
                busca(b, at + 4, b"-->").map_or(b.len(), |p| p + 3)
            };
            continue;
        }
        if matches!(b.get(at + 1), Some(b'!' | b'?')) {
            i = busca(b, at + 2, b">").map_or(b.len(), |p| p + 1);
            continue;
        }
        let fecha = b.get(at + 1) == Some(&b'/');
        let ini = at + 1 + usize::from(fecha);
        let tam = b[ini..].iter().take_while(|c| c.is_ascii_alphanumeric() || **c == b'-').count();
        if tam == 0 {
            i = at + 1;
            continue;
        }
        let nome = b[ini..ini + tam].to_ascii_lowercase();
        i = fim_da_tag(b, ini + tam);
        if fecha {
            fecha_elemento(&mut pilha, &mut abertos, &nome);
            continue;
        }
        if esta(VAZIAS, &nome) {
            continue;
        }
        if esta(CRUS, &nome) {
            let mut fechamento = b"</".to_vec();
            fechamento.extend_from_slice(&nome);
            i = busca(b, i, &fechamento).map_or(b.len(), |p| fim_da_tag(b, p + fechamento.len()));
            continue;
        }
        if pilha.len() >= MAX_HTML_PARSE_DEPTH {
            return Err(MSG_ANINHADO.to_string());
        }
        *abertos.entry(nome.clone()).or_insert(0) += 1;
        pilha.push(nome);
    }
    Ok(())
}

/// Fechamento de `nome`: só vale se há elemento aberto com esse nome (senão o parser o ignora);
/// fecha tudo o que está acima dele na pilha. Cada elemento é desempilhado uma vez: tempo linear.
fn fecha_elemento(pilha: &mut Vec<Vec<u8>>, abertos: &mut HashMap<Vec<u8>, usize>, nome: &[u8]) {
    if abertos.get(nome).copied().unwrap_or(0) == 0 {
        return;
    }
    while let Some(topo) = pilha.pop() {
        if let Some(n) = abertos.get_mut(&topo) {
            *n -= 1;
        }
        if topo == nome {
            break;
        }
    }
}

fn esta(lista: &[&str], nome: &[u8]) -> bool {
    lista.iter().any(|v| v.as_bytes() == nome)
}

/// Posição de `pat` (sem diferenciar caixa) a partir de `de`.
fn busca(b: &[u8], de: usize, pat: &[u8]) -> Option<usize> {
    b[de..].windows(pat.len()).position(|w| w.eq_ignore_ascii_case(pat)).map(|p| p + de)
}

/// Posição logo depois do `>` que fecha a tag, respeitando valores de atributo entre aspas.
/// Aspas só abrem valor logo depois de `=` (como no tokenizador do HTML); sem `>` até o fim, vai ao fim.
fn fim_da_tag(b: &[u8], de: usize) -> usize {
    let mut j = de;
    while j < b.len() {
        match b[j] {
            b'>' => return j + 1,
            b'=' => {
                let mut k = j + 1;
                while k < b.len() && b[k].is_ascii_whitespace() {
                    k += 1;
                }
                if let Some(&q @ (b'"' | b'\'')) = b.get(k) {
                    match b[k + 1..].iter().position(|&c| c == q) {
                        Some(p) => j = k + 1 + p,
                        None => return b.len(),
                    }
                }
            }
            _ => {}
        }
        j += 1;
    }
    b.len()
}

#[cfg(test)]
mod tests {
    use super::confere;

    #[test]
    fn fechamento_sem_abertura_nao_diminui_a_conta() {
        let h = "<center></foo>".repeat(6000);
        assert!(confere(&h).is_err());
    }

    #[test]
    fn vazias_e_texto_cru_nao_contam() {
        let h = "<br><img src=x>".repeat(6000) + &"<script>a<b<c</script>".repeat(6000);
        assert!(confere(&h).is_ok());
    }

    #[test]
    fn valor_entre_aspas_com_maior_que_nao_fecha_tag() {
        assert!(confere(r#"<a title="<div>"><p>x</p>"#).is_ok());
    }
}
