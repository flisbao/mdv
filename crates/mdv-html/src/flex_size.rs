//! Aritmética do flex de linha, sem renderização: quanto cada item ocupa quando a soma das
//! larguras-alvo não cabe (encolhe, sem passar do mínimo), quando sobra espaço (cresce pelo
//! `flex-grow`, ou se distribui pelo `justify-content`), e a largura natural de um bloco já
//! renderizado. Tudo em colunas.

use crate::model::Line;
use crate::style::Justify;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// Medidas de um item de flex, em colunas.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Dimensao {
    /// Largura-alvo: `flex-basis`, `width` ou a largura natural.
    pub alvo: usize,
    /// Menor largura em que o item ainda cabe (maior palavra, no mínimo 3 colunas).
    pub minimo: usize,
    /// `flex-grow`: a fatia da sobra que o item recebe (0 = não cresce).
    pub grow: f32,
    /// Largura fixa (`width` ou `flex-basis` sem crescimento): não encolhe.
    pub fixo: bool,
}

/// Larguras decididas para cada item, com os espaços de `justify-content` (quando não há crescimento).
#[derive(Debug, PartialEq)]
pub(crate) struct Plano {
    pub larguras: Vec<usize>,
    /// Espaço antes do primeiro item.
    pub inicio: usize,
    /// Espaço extra entre cada par de itens (um valor a menos que o número de itens).
    pub interno: Vec<usize>,
    /// Espaço depois do último item.
    pub fim: usize,
}

/// Planeja as larguras de `itens` numa linha de `w` colunas, com `vaos` colunas de espaçamento
/// (`gap`). Sobra: quem tem `grow` divide-a na proporção dele; sem crescimento, `j` a distribui.
/// Falta: os itens não fixos encolhem em proporção ao alvo, sem passar do mínimo. Devolve `None`
/// quando nem os mínimos cabem: aí o contêiner empilha os itens.
pub(crate) fn planeja(itens: &[Dimensao], vaos: usize, w: usize, j: Justify) -> Option<Plano> {
    let n = itens.len();
    let livre = w.checked_sub(vaos)?;
    let alvos: Vec<usize> = itens.iter().map(|d| d.alvo.min(w)).collect();
    let soma: usize = alvos.iter().sum();
    if soma <= livre {
        let sobra = livre - soma;
        if itens.iter().any(|d| d.grow > 0.0) {
            let pesos: Vec<f64> = itens.iter().map(|d| f64::from(d.grow.max(0.0))).collect();
            let larguras = alvos
                .iter()
                .zip(reparte(sobra, &pesos))
                .map(|(a, p)| a + p)
                .collect();
            return Some(Plano {
                larguras,
                inicio: 0,
                interno: vec![0; n.saturating_sub(1)],
                fim: 0,
            });
        }
        let (inicio, interno, fim) = distribui(j, n, sobra);
        return Some(Plano {
            larguras: alvos,
            inicio,
            interno,
            fim,
        });
    }
    // Encolhe: os fixos mantêm a largura; os demais repartem o resto em proporção ao alvo,
    // sem ficar abaixo do mínimo (quem fica abaixo é travado no mínimo e sai da conta).
    let minimos: Vec<usize> = itens
        .iter()
        .zip(&alvos)
        .map(|(d, &a)| if d.fixo { a } else { d.minimo.min(w) })
        .collect();
    if minimos.iter().sum::<usize>() > livre {
        return None;
    }
    let fixos: usize = itens
        .iter()
        .zip(&alvos)
        .filter(|(d, _)| d.fixo)
        .map(|(_, &a)| a)
        .sum();
    let mut restante = livre.saturating_sub(fixos);
    let mut larguras = alvos.clone();
    let mut ativos: Vec<usize> = itens
        .iter()
        .enumerate()
        .filter(|(_, d)| !d.fixo)
        .map(|(k, _)| k)
        .collect();
    while !ativos.is_empty() {
        let pesos: Vec<f64> = ativos
            .iter()
            .map(|&k| alvos.get(k).copied().unwrap_or(0).max(1) as f64)
            .collect();
        let partes = reparte(restante, &pesos);
        let minimo_de = |k: usize| minimos.get(k).copied().unwrap_or(0);
        let abaixo: Vec<usize> = ativos
            .iter()
            .zip(&partes)
            .filter(|&(&k, &p)| p < minimo_de(k))
            .map(|(&k, _)| k)
            .collect();
        if abaixo.is_empty() {
            for (&k, p) in ativos.iter().zip(partes) {
                if let Some(l) = larguras.get_mut(k) {
                    *l = p;
                }
            }
            break;
        }
        for &k in &abaixo {
            let m = minimo_de(k);
            if let Some(l) = larguras.get_mut(k) {
                *l = m;
            }
            restante = restante.saturating_sub(m);
        }
        ativos.retain(|k| !abaixo.contains(k));
    }
    Some(Plano {
        larguras,
        inicio: 0,
        interno: vec![0; n.saturating_sub(1)],
        fim: 0,
    })
}

/// Reparte `total` em partes proporcionais a `pesos` (a soma das partes é sempre `total`). Sem
/// peso positivo, as partes são iguais. O resto da divisão vai para os primeiros.
pub(crate) fn reparte(total: usize, pesos: &[f64]) -> Vec<usize> {
    let n = pesos.len();
    if n == 0 {
        return Vec::new();
    }
    let soma: f64 = pesos.iter().sum();
    let iguais = soma.is_nan() || soma <= 0.0;
    let base = if iguais { n as f64 } else { soma };
    let mut partes: Vec<usize> = pesos
        .iter()
        .map(|&p| {
            let q = if iguais { 1.0 } else { p.max(0.0) };
            (total as f64 * q / base).floor().max(0.0) as usize
        })
        .collect();
    let resto = total.saturating_sub(partes.iter().sum());
    for k in 0..resto {
        if let Some(p) = partes.get_mut(k % n) {
            *p += 1;
        }
    }
    partes
}

/// Espaços de `justify-content` para `n` itens e `livre` colunas: (início, entre os itens, fim).
/// A soma das três partes é sempre `livre`.
pub(crate) fn distribui(j: Justify, n: usize, livre: usize) -> (usize, Vec<usize>, usize) {
    let vaos = n.saturating_sub(1);
    match j {
        Justify::End => (livre, vec![0; vaos], 0),
        Justify::Center => {
            let ini = livre / 2;
            (ini, vec![0; vaos], livre - ini)
        }
        Justify::Between if vaos > 0 => {
            let base = livre / vaos;
            let resto = livre % vaos;
            let internos = (0..vaos).map(|i| base + usize::from(i < resto)).collect();
            (0, internos, 0)
        }
        Justify::Around if n > 0 => {
            let s = livre / n;
            let ini = s / 2;
            (ini, vec![s; vaos], livre - ini - s * vaos)
        }
        _ => (0, vec![0; vaos], livre),
    }
}

/// Largura natural de um item: a largura da maior linha até o último caractere que não é espaço
/// (o preenchimento à direita não conta). Mínimo 1.
pub(crate) fn largura_natural(linhas: &[Line]) -> usize {
    linhas
        .iter()
        .map(|l| {
            let mut acumulada = 0usize;
            let mut fim = 0usize;
            for c in l {
                acumulada += UnicodeWidthChar::width(c.ch).unwrap_or(0);
                if c.ch != ' ' {
                    fim = acumulada;
                }
            }
            fim
        })
        .max()
        .unwrap_or(0)
        .max(1)
}

/// Menor largura, em colunas, em que `texto` ainda não quebra uma palavra: a da maior palavra.
pub(crate) fn maior_palavra(texto: &str) -> usize {
    texto
        .split_whitespace()
        .map(|p| p.width())
        .max()
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dim(alvo: usize, minimo: usize) -> Dimensao {
        Dimensao {
            alvo,
            minimo,
            grow: 0.0,
            fixo: false,
        }
    }

    #[test]
    fn reparte_soma_o_total() {
        assert_eq!(reparte(10, &[1.0, 2.0]), vec![4, 6]);
        assert_eq!(reparte(7, &[0.0, 0.0, 0.0]).iter().sum::<usize>(), 7);
        assert!(reparte(5, &[]).is_empty());
    }

    #[test]
    fn planeja_cresce_pela_sobra() {
        let mut a = dim(0, 3);
        a.grow = 1.0;
        let mut b = dim(0, 3);
        b.grow = 2.0;
        let p = planeja(&[a, b], 0, 30, Justify::Start);
        assert_eq!(p.map(|p| p.larguras), Some(vec![10, 20]));
    }

    #[test]
    fn planeja_justify_sem_crescimento() {
        let p = planeja(&[dim(10, 3), dim(10, 3)], 0, 30, Justify::End);
        let p = p.unwrap_or(Plano {
            larguras: Vec::new(),
            inicio: 0,
            interno: Vec::new(),
            fim: 0,
        });
        assert_eq!(p.larguras, vec![10, 10]);
        assert_eq!(p.inicio, 10);
    }

    #[test]
    fn planeja_encolhe_em_proporcao_e_soma_exato() {
        let p = planeja(&[dim(40, 7), dim(40, 7), dim(40, 7)], 2, 100, Justify::Start);
        let larguras = p.map(|p| p.larguras).unwrap_or_default();
        assert_eq!(larguras.iter().sum::<usize>(), 98);
        assert!(larguras.iter().all(|&l| l >= 7));
    }

    #[test]
    fn planeja_respeita_o_minimo() {
        // Sem o mínimo, o segundo item ficaria com 1 coluna; ele sobe para 8 e o primeiro encolhe.
        let p = planeja(&[dim(100, 3), dim(10, 8)], 0, 20, Justify::Start);
        assert_eq!(p.map(|p| p.larguras), Some(vec![12, 8]));
    }

    #[test]
    fn planeja_devolve_none_quando_minimos_nao_cabem() {
        let itens = [dim(20, 5), dim(20, 5), dim(20, 5)];
        assert_eq!(planeja(&itens, 0, 12, Justify::Start), None);
    }

    #[test]
    fn planeja_fixo_mantem_a_largura() {
        let mut fixo = dim(20, 20);
        fixo.fixo = true;
        let p = planeja(&[fixo, dim(100, 3)], 0, 30, Justify::Start);
        assert_eq!(p.map(|p| p.larguras), Some(vec![20, 10]));
    }

    #[test]
    fn maior_palavra_ignora_espacos() {
        assert_eq!(maior_palavra("um elefante  grande"), 8);
        assert_eq!(maior_palavra("   "), 0);
    }
}
