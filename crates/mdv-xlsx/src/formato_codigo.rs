//! Código de formato (id embutido ou texto personalizado) → `Formato`.
//! Id embutido segue a tabela do Excel; código personalizado é lido pelas peças de `formato_literais`.
//! Ordem de decisão do personalizado: texto (`@`), científico e fração (viram Geral), data,
//! percentual e número. Sem marcador nenhum, sobra Geral.

use crate::formato::Formato;
use crate::formato_literais::{tokenizar, Peca};

pub(crate) fn classificar(id_embutido: u32, codigo: Option<&str>) -> Formato {
    match codigo {
        Some(texto) => personalizado(texto),
        None => embutido(id_embutido),
    }
}

fn numero(decimais: u8, milhar: bool) -> Formato {
    Formato::Numero { decimais, milhar, prefixo: String::new(), sufixo: String::new() }
}

/// Ids embutidos do Excel. Os não listados (fração 12–13, científico 11 e 48, ids reservados)
/// caem em Geral. Os ids de moeda 5–8 e contábeis 41–44 saem como número, sem o símbolo.
fn embutido(id: u32) -> Formato {
    match id {
        1 => numero(0, false),
        2 => numero(2, false),
        3 => numero(0, true),
        4 => numero(2, true),
        5 | 6 | 37 | 38 | 41 | 42 => numero(0, true),
        7 | 8 | 39 | 40 | 43 | 44 => numero(2, true),
        9 => Formato::Percentual { decimais: 0 },
        10 => Formato::Percentual { decimais: 2 },
        14..=17 | 27..=36 | 50..=58 => Formato::Data { data: true, hora: false, segundos: false },
        18 | 20 => Formato::Data { data: false, hora: true, segundos: false },
        19 | 21 | 45..=47 => Formato::Data { data: false, hora: true, segundos: true },
        22 => Formato::Data { data: true, hora: true, segundos: false },
        49 => Formato::Texto,
        _ => Formato::Geral,
    }
}

fn personalizado(codigo: &str) -> Formato {
    let pecas = tokenizar(codigo);
    if pecas.contains(&Peca::Texto) {
        return Formato::Texto;
    }
    if pecas.contains(&Peca::Cientifico) || tem_fracao(&pecas) {
        return Formato::Geral;
    }
    if let Some(data) = data_de(&pecas) {
        return data;
    }
    if !pecas.iter().any(|p| matches!(p, Peca::Marcador(_))) {
        return Formato::Geral;
    }
    let decimais = decimais(&pecas);
    if pecas.contains(&Peca::Percentual) {
        return Formato::Percentual { decimais };
    }
    numero_personalizado(&pecas, decimais)
}

/// Um marcador seguido de barra (`# ?/?`) é fração; o Excel mostra fração e Geral não a imita.
fn tem_fracao(pecas: &[Peca]) -> bool {
    pecas.windows(2).any(|w| matches!(w, [Peca::Marcador(_), Peca::Barra]))
}

/// Data se há `y`, `d` ou `m` (que sem hora é mês); hora se há `h` ou `s` (e `m` é minuto).
fn data_de(pecas: &[Peca]) -> Option<Formato> {
    let tem = |c: char| pecas.contains(&Peca::Data(c));
    let hora = tem('h') || tem('s');
    let data = tem('y') || tem('d') || (tem('m') && !hora);
    (data || hora).then_some(Formato::Data { data, hora, segundos: tem('s') })
}

/// Marcadores depois do ponto decimal, no máximo 15.
fn decimais(pecas: &[Peca]) -> u8 {
    let depois_do_ponto = pecas
        .iter()
        .skip_while(|p| **p != Peca::Ponto)
        .skip(1)
        .take_while(|p| matches!(p, Peca::Marcador(_)))
        .count();
    depois_do_ponto.min(15) as u8
}

/// Prefixo e sufixo são o literal antes do primeiro marcador e depois do último; milhar é a
/// vírgula entre marcadores.
fn numero_personalizado(pecas: &[Peca], decimais: u8) -> Formato {
    let marcador = |p: &Peca| matches!(p, Peca::Marcador(_));
    let primeiro = pecas.iter().position(marcador).unwrap_or(0);
    let ultimo = pecas.iter().rposition(marcador).unwrap_or(0);
    Formato::Numero {
        decimais,
        milhar: pecas[primeiro..=ultimo].contains(&Peca::Virgula),
        prefixo: texto_literal(&pecas[..primeiro]),
        sufixo: texto_literal(&pecas[ultimo + 1..]),
    }
}

fn texto_literal(pecas: &[Peca]) -> String {
    pecas
        .iter()
        .filter_map(|p| match p {
            Peca::Literal(t) => Some(t.as_str()),
            _ => None,
        })
        .collect()
}
