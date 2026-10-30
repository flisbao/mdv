//! Agrupa blocos consecutivos do mesmo tipo (itens, código, citações) antes de escrever.

use crate::modelo::Bloco;

/// Blocos consecutivos que são escritos juntos.
#[derive(Debug, PartialEq)]
pub(crate) enum Grupo<'a> {
    /// Título, parágrafo ou tabela, escrito sozinho.
    Bloco(&'a Bloco),
    /// Itens de lista seguidos.
    Lista(&'a [Bloco]),
    /// Blocos de código seguidos, numa só cerca.
    Codigo(&'a [Bloco]),
    /// Parágrafos de citação seguidos.
    Citacao(&'a [Bloco]),
}

/// Agrupa `Item`, `Codigo` e `Citacao` consecutivos, preservando a ordem.
///
/// Os itens de uma lista precisam sair colados (sem linha vazia entre eles) e os blocos de
/// código seguidos vão numa cerca só; por isso o escritor recebe a sequência inteira, e não
/// um bloco por vez. Título, parágrafo e tabela sempre saem sozinhos.
pub(crate) fn agrupar(blocos: &[Bloco]) -> Vec<Grupo<'_>> {
    let mut grupos = Vec::new();
    let mut inicio = 0;
    while inicio < blocos.len() {
        let atual = &blocos[inicio];
        let fim = inicio + sequencia(&blocos[inicio..]);
        grupos.push(match atual {
            Bloco::Item { .. } => Grupo::Lista(&blocos[inicio..fim]),
            Bloco::Codigo(_) => Grupo::Codigo(&blocos[inicio..fim]),
            Bloco::Citacao(_) => Grupo::Citacao(&blocos[inicio..fim]),
            _ => Grupo::Bloco(atual),
        });
        inicio = fim;
    }
    grupos
}

/// Quantos blocos, a partir do primeiro, são do mesmo tipo agrupável (1 se não for agrupável).
fn sequencia(blocos: &[Bloco]) -> usize {
    let mesmo = |a: &Bloco, b: &Bloco| {
        matches!(
            (a, b),
            (Bloco::Item { .. }, Bloco::Item { .. })
                | (Bloco::Codigo(_), Bloco::Codigo(_))
                | (Bloco::Citacao(_), Bloco::Citacao(_))
        )
    };
    blocos.iter().take_while(|b| mesmo(&blocos[0], b)).count().max(1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modelo::{Marcas, Trecho};

    fn texto(s: &str) -> Vec<Trecho> {
        vec![Trecho::Texto { texto: s.into(), marcas: Marcas::default(), link: None }]
    }

    fn item(s: &str) -> Bloco {
        Bloco::Item { nivel: 0, numerada: false, numero: 0, texto: texto(s) }
    }

    #[test]
    fn itens_seguidos_formam_uma_lista() {
        let blocos = [item("a"), item("b"), Bloco::Paragrafo(texto("p"))];
        let grupos = agrupar(&blocos);
        assert_eq!(grupos.len(), 2);
        assert_eq!(grupos[0], Grupo::Lista(&blocos[..2]));
        assert_eq!(grupos[1], Grupo::Bloco(&blocos[2]));
    }

    #[test]
    fn codigos_seguidos_formam_um_grupo() {
        let blocos = [Bloco::Codigo("a".into()), Bloco::Codigo("b".into())];
        assert_eq!(agrupar(&blocos), [Grupo::Codigo(&blocos[..])]);
    }

    #[test]
    fn citacoes_seguidas_formam_um_grupo() {
        let blocos = [Bloco::Citacao(texto("a")), Bloco::Citacao(texto("b"))];
        assert_eq!(agrupar(&blocos), [Grupo::Citacao(&blocos[..])]);
    }

    #[test]
    fn ordem_e_preservada_em_blocos_misturados() {
        let blocos = [
            Bloco::Paragrafo(texto("p")),
            item("a"),
            item("b"),
            Bloco::Paragrafo(texto("q")),
            Bloco::Codigo("c".into()),
        ];
        let grupos = agrupar(&blocos);
        assert_eq!(
            grupos,
            [
                Grupo::Bloco(&blocos[0]),
                Grupo::Lista(&blocos[1..3]),
                Grupo::Bloco(&blocos[3]),
                Grupo::Codigo(&blocos[4..]),
            ]
        );
    }
}
