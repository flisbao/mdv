//! Dobra de seções: quais linhas renderizadas aparecem quando alguns títulos estão dobrados.

use std::collections::{HashMap, HashSet};

use crate::render::{section_range, OutlineEntry, Rendered};

/// (título, n-ésima ocorrência do mesmo título): estável em reload e mudança de largura.
pub type FoldKey = (String, usize);

/// Uma chave por entrada de `outline`, na mesma ordem.
pub fn fold_keys(outline: &[OutlineEntry]) -> Vec<FoldKey> {
    let mut seen: HashMap<&str, usize> = HashMap::new();
    outline
        .iter()
        .map(|e| {
            let n = seen.entry(e.title.as_str()).or_insert(0);
            let key = (e.title.clone(), *n);
            *n += 1;
            key
        })
        .collect()
}

/// Linhas visíveis (índices em `Rendered.lines`) e, para cada título dobrado visível,
/// quantas linhas ele esconde.
pub struct Display {
    pub lines: Vec<usize>,
    pub hidden: HashMap<usize, usize>,
}

pub fn compute(r: &Rendered, folded: &HashSet<FoldKey>) -> Display {
    let n = r.lines.len();
    let mut hidden_line = vec![false; n];
    let mut hidden = HashMap::new();
    if !folded.is_empty() {
        for (i, key) in fold_keys(&r.outline).iter().enumerate() {
            let line = r.outline[i].line;
            if !folded.contains(key) || line >= n || hidden_line[line] {
                continue;
            }
            let (start, end) = section_range(&r.outline, i, n);
            let end = end.min(n);
            if end > start + 1 {
                hidden_line[start + 1..end].fill(true);
                hidden.insert(start, end - start - 1);
            }
        }
    }
    let lines = (0..n).filter(|&l| !hidden_line[l]).collect();
    Display { lines, hidden }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::{render, section_range};

    const DOC: &str = "# A\n\ntexto A\n\n## B\n\ntexto B\n\n## C\n\ntexto C\n\n# D\n\ntexto D\n";

    fn doc() -> Rendered {
        render(DOC, 40)
    }

    fn pastas(r: &Rendered, titulos: &[&str]) -> HashSet<FoldKey> {
        let keys = fold_keys(&r.outline);
        r.outline
            .iter()
            .zip(keys)
            .filter(|(e, _)| titulos.contains(&e.title.as_str()))
            .map(|(_, k)| k)
            .collect()
    }

    #[test]
    fn chaves_numeram_as_ocorrencias_do_mesmo_titulo() {
        let r = render("# A\n\n## x\n\n## x\n\n# x\n", 40);
        assert_eq!(
            fold_keys(&r.outline),
            vec![("A".to_string(), 0), ("x".to_string(), 0), ("x".to_string(), 1), ("x".to_string(), 2)]
        );
    }

    #[test]
    fn chave_sobrevive_a_edicao_em_outro_lugar() {
        let antes = render("# A\n\ntexto\n\n## B\n", 40);
        let depois = render("# A\n\ntexto\n\nmais um parágrafo\n\n## B\n", 40);
        assert_eq!(fold_keys(&antes.outline)[1], fold_keys(&depois.outline)[1]);
    }

    #[test]
    fn sem_dobras_todas_as_linhas_aparecem() {
        let r = doc();
        let d = compute(&r, &HashSet::new());
        assert_eq!(d.lines, (0..r.lines.len()).collect::<Vec<_>>());
        assert!(d.hidden.is_empty());
    }

    #[test]
    fn dobrar_um_titulo_esconde_o_corpo_e_mantem_o_titulo() {
        let r = doc();
        let d = compute(&r, &pastas(&r, &["B"]));
        let (s, e) = section_range(&r.outline, 1, r.lines.len());
        assert!(d.lines.contains(&s), "o título dobrado continua visível");
        assert!((s + 1..e).all(|l| !d.lines.contains(&l)));
        assert_eq!(d.hidden.get(&s), Some(&(e - s - 1)));
        let (cs, _) = section_range(&r.outline, 2, r.lines.len());
        assert!(d.lines.contains(&cs), "a seção irmã não é afetada");
    }

    #[test]
    fn dobrar_o_pai_esconde_os_filhos_e_so_o_pai_leva_o_sufixo() {
        let r = doc();
        let d = compute(&r, &pastas(&r, &["A", "B"]));
        let (a_s, a_e) = section_range(&r.outline, 0, r.lines.len());
        assert!((a_s + 1..a_e).all(|l| !d.lines.contains(&l)));
        assert!(d.hidden.contains_key(&a_s));
        assert!(!d.hidden.contains_key(&r.outline[1].line), "B está escondido; sem sufixo");
        let (d_s, _) = section_range(&r.outline, 3, r.lines.len());
        assert!(d.lines.contains(&d_s), "o título seguinte de mesmo nível continua visível");
    }

    #[test]
    fn abrir_o_pai_deixa_o_filho_dobrado() {
        let r = doc();
        let d = compute(&r, &pastas(&r, &["B"]));
        let (b_s, b_e) = section_range(&r.outline, 1, r.lines.len());
        assert!(d.lines.contains(&r.outline[0].line));
        assert!((b_s + 1..b_e).all(|l| !d.lines.contains(&l)), "B continua dobrado");
    }

    #[test]
    fn chave_de_titulo_que_sumiu_e_ignorada() {
        let r = doc();
        let mut f = HashSet::new();
        f.insert(("não existe".to_string(), 0));
        assert_eq!(compute(&r, &f).lines.len(), r.lines.len());
    }

    #[test]
    fn dois_titulos_iguais_dobram_separados() {
        let r = render("# x\n\na\n\n# x\n\nb\n", 40);
        let mut f = HashSet::new();
        f.insert(fold_keys(&r.outline)[1].clone());
        let d = compute(&r, &f);
        assert!(d.lines.contains(&r.outline[0].line) && d.lines.contains(&(r.outline[0].line + 2)));
        assert!(d.hidden.contains_key(&r.outline[1].line));
        assert!(!d.hidden.contains_key(&r.outline[0].line));
    }

    #[test]
    fn documento_sem_titulos_nao_muda() {
        let r = render("só texto\n\noutro\n", 40);
        let d = compute(&r, &HashSet::new());
        assert_eq!(d.lines.len(), r.lines.len());
    }
}
