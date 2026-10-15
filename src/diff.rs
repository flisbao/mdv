//! Diferença entre duas versões do fonte, para marcar o que mudou no reload.

use std::collections::BTreeSet;
use std::time::Duration;

use similar::{Algorithm, ChangeTag, TextDiff};

/// Acima disso (em qualquer das versões) o diff não é calculado.
pub const MAX_DIFF_LINES: usize = 20_000;

/// Prazo do diff: passado dele, `similar` devolve um resultado grosseiro mas válido.
const DIFF_TIMEOUT: Duration = Duration::from_millis(300);

/// Linhas (0-based) de `new` que são inserções ou substituições em relação a `old`.
/// `None` se alguma das versões passa de `MAX_DIFF_LINES` linhas.
pub fn changed_lines(old: &str, new: &str) -> Option<BTreeSet<usize>> {
    if old.lines().count() > MAX_DIFF_LINES || new.lines().count() > MAX_DIFF_LINES {
        return None;
    }
    let diff = TextDiff::configure()
        .algorithm(Algorithm::Myers)
        .timeout(DIFF_TIMEOUT)
        .diff_lines(old, new);
    let mut out = BTreeSet::new();
    for change in diff.iter_all_changes() {
        if change.tag() == ChangeTag::Insert {
            out.extend(change.new_index());
        }
    }
    Some(out)
}

/// Uma marca por linha renderizada: a linha é marcada se alguma linha do fonte do seu bloco
/// (da `line_src` dela até a do próximo bloco) mudou.
pub fn marks(line_src: &[usize], changed: &BTreeSet<usize>) -> Vec<bool> {
    let n = line_src.len();
    if changed.is_empty() {
        return vec![false; n];
    }
    // `next[i]`: primeira linha do fonte, depois da linha renderizada `i`, maior que a dela.
    let mut next = vec![usize::MAX; n];
    for i in (0..n.saturating_sub(1)).rev() {
        next[i] = if line_src[i + 1] > line_src[i] { line_src[i + 1] } else { next[i + 1] };
    }
    (0..n).map(|i| changed.range(line_src[i]..next[i]).next().is_some()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(v: &[usize]) -> BTreeSet<usize> {
        v.iter().copied().collect()
    }

    #[test]
    fn insercao_marca_a_linha_nova() {
        assert_eq!(changed_lines("a\nb\n", "a\nx\nb\n").unwrap(), set(&[1]));
    }

    #[test]
    fn edicao_marca_a_linha_substituida() {
        assert_eq!(changed_lines("a\nb\nc\n", "a\nB\nc\n").unwrap(), set(&[1]));
    }

    #[test]
    fn remocao_pura_nao_marca_nada() {
        assert!(changed_lines("a\nb\nc\n", "a\nc\n").unwrap().is_empty());
    }

    #[test]
    fn texto_igual_nao_marca_nada() {
        assert!(changed_lines("a\nb\n", "a\nb\n").unwrap().is_empty());
    }

    #[test]
    fn acrescentar_no_fim_marca_as_linhas_novas() {
        assert_eq!(changed_lines("a\n", "a\nb\nc\n").unwrap(), set(&[1, 2]));
    }

    #[test]
    fn documento_grande_demais_nao_calcula() {
        let grande = "x\n".repeat(MAX_DIFF_LINES + 1);
        assert!(changed_lines(&grande, "x\n").is_none());
        assert!(changed_lines("x\n", &grande).is_none());
        let no_teto = "x\n".repeat(MAX_DIFF_LINES);
        assert!(changed_lines(&no_teto, &no_teto).is_some());
    }

    #[test]
    fn marks_pinta_todas_as_linhas_do_bloco_quebrado() {
        // Parágrafo da linha 0 do fonte quebrado em 3 linhas renderizadas; o seguinte começa na linha 3.
        let line_src = [0, 0, 0, 3, 3, 5];
        assert_eq!(marks(&line_src, &set(&[1])), vec![true, true, true, false, false, false]);
        assert_eq!(marks(&line_src, &set(&[4])), vec![false, false, false, true, true, false]);
    }

    #[test]
    fn marks_de_lista_aninhada_pinta_so_o_item() {
        assert_eq!(marks(&[0, 1, 2], &set(&[1])), vec![false, true, false]);
    }

    #[test]
    fn marks_sem_mudancas_ou_sem_mapa_e_tudo_falso_ou_vazio() {
        assert_eq!(marks(&[0, 1], &BTreeSet::new()), vec![false, false]);
        assert!(marks(&[], &set(&[0])).is_empty());
    }

    #[test]
    fn marks_aceita_linha_de_fonte_alem_do_fim_do_mapa() {
        assert_eq!(marks(&[0, 2], &set(&[9])), vec![false, true]);
    }
}
