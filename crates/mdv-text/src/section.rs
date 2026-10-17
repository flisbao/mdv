//! Busca de seção por título, o intervalo (em bytes) que ela ocupa no fonte e a linha `<summary>`.

use std::ops::Range;

use crate::{sanitize, Heading};

/// Índice do título pedido: igualdade sem caixa, senão substring única, senão erro.
pub fn find_section(heads: &[Heading], query: &str) -> Result<usize, String> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return Err("o título da seção está vazio".to_string());
    }
    if let Some(i) = heads.iter().position(|h| h.title.to_lowercase() == q) {
        return Ok(i);
    }
    let parecidos: Vec<usize> = (0..heads.len()).filter(|&i| heads[i].title.to_lowercase().contains(&q)).collect();
    match parecidos.as_slice() {
        [] => Err(format!("nenhuma seção com o título \"{}\"", query.trim())),
        [i] => Ok(*i),
        varios => {
            let nomes: Vec<String> = varios.iter().map(|&i| sanitize(&heads[i].title).into_owned()).collect();
            Err(format!("o título \"{}\" é ambíguo; candidatos: {}", query.trim(), nomes.join(", ")))
        }
    }
}

/// Bytes da seção `idx`: do título até o próximo de nível igual ou menor (ou o fim do fonte).
pub fn section_range(md: &str, heads: &[Heading], idx: usize) -> Range<usize> {
    let h = &heads[idx];
    let end = heads[idx + 1..].iter().find(|n| n.level <= h.level).map_or(md.len(), |n| n.start);
    h.start..end
}

/// Texto de uma linha `<summary>…</summary>` inteira, sem as marcas.
pub fn summary_text(line: &str) -> Option<&str> {
    line.trim().strip_prefix("<summary>")?.strip_suffix("</summary>").map(str::trim)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::headings;

    #[test]
    fn find_section_prefere_igualdade_sem_caixa() {
        let h = headings("# Instalar\n\n# Instalar tudo\n");
        assert_eq!(find_section(&h, "instalar").unwrap(), 0);
    }

    #[test]
    fn find_section_aceita_substring_unica() {
        let h = headings("# Guia de uso\n\n# Notas\n");
        assert_eq!(find_section(&h, "uso").unwrap(), 0);
    }

    #[test]
    fn find_section_ambigua_lista_os_candidatos() {
        let h = headings("# Passo um\n\n# Passo dois\n");
        let e = find_section(&h, "passo").unwrap_err();
        assert!(e.contains("Passo um") && e.contains("Passo dois"), "{e}");
    }

    #[test]
    fn find_section_inexistente_ou_vazio_e_erro() {
        let h = headings("# A\n");
        assert!(find_section(&h, "zzz").unwrap_err().contains("zzz"));
        assert!(find_section(&h, "   ").is_err());
        assert!(find_section(&[], "a").is_err());
    }

    #[test]
    fn section_range_vai_ate_o_proximo_titulo_de_nivel_igual_ou_menor() {
        let md = "intro\n\n# A\n\ntexto\n\n## B\n\nb\n\n# C\n\nc\n";
        let h = headings(md);
        assert_eq!(&md[section_range(md, &h, 0)], "# A\n\ntexto\n\n## B\n\nb\n\n");
        assert_eq!(&md[section_range(md, &h, 1)], "## B\n\nb\n\n");
        assert_eq!(&md[section_range(md, &h, 2)], "# C\n\nc\n");
    }

    #[test]
    fn summary_text_so_aceita_a_linha_inteira() {
        assert_eq!(summary_text("<summary> Detalhes </summary>"), Some("Detalhes"));
        assert_eq!(summary_text("  <summary>x</summary>  "), Some("x"));
        assert_eq!(summary_text("<summary>x"), None);
        assert_eq!(summary_text("texto"), None);
    }
}
