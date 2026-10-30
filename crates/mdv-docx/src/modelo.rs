//! Modelo intermediário do documento: blocos, trechos, marcas e tabelas já com o sentido do Word.
//! Não conhece XML nem Markdown; quem lê preenche e quem escreve consome.

use std::collections::HashMap;

/// Documento lido: blocos na ordem do corpo e o texto de cada nota (rodapé ou fim), por chave.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Documento {
    pub blocos: Vec<Bloco>,
    pub notas: HashMap<String, Vec<Trecho>>,
}

/// Bloco de primeiro nível do corpo.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Bloco {
    /// Título de nível 1 a 6.
    Titulo { nivel: u8, texto: Vec<Trecho> },
    /// Parágrafo comum.
    Paragrafo(Vec<Trecho>),
    /// Item de lista; `numero` é 0 quando a lista não é numerada.
    Item { nivel: u8, numerada: bool, numero: u32, texto: Vec<Trecho> },
    /// Bloco de código, já com as quebras de linha.
    Codigo(String),
    /// Parágrafo de citação.
    Citacao(Vec<Trecho>),
    /// Tabela de linhas e células.
    Tabela(Tabela),
}

/// Pedaço de texto em linha, quebra, referência de nota ou imagem.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Trecho {
    /// Texto com marcas e, se for link, o destino.
    Texto { texto: String, marcas: Marcas, link: Option<String> },
    /// Quebra de linha dentro do parágrafo (`w:br`).
    Quebra,
    /// Chave da nota: `n<id>` para rodapé, `e<id>` para nota de fim.
    NotaRef(String),
    /// Imagem, guardada só pelo texto alternativo (pode ser vazio).
    Imagem(String),
}

/// Marcas de caractere de um trecho.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct Marcas {
    pub negrito: bool,
    pub italico: bool,
    pub tachado: bool,
    pub codigo: bool,
}

/// Tabela como linhas de células; a primeira linha é o cabeçalho na saída.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Tabela {
    pub linhas: Vec<Vec<Celula>>,
}

/// Célula: seus parágrafos, quantas colunas ela ocupa e se é continuação de uma mesclagem vertical.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Celula {
    pub paragrafos: Vec<Vec<Trecho>>,
    pub colunas: usize,
    pub continua: bool,
}

impl Bloco {
    /// Parágrafos do bloco na ordem de leitura; código vira um parágrafo marcado e tabela achata as células.
    pub(crate) fn paragrafos(self) -> Vec<Vec<Trecho>> {
        match self {
            Bloco::Titulo { texto, .. } | Bloco::Item { texto, .. } => vec![texto],
            Bloco::Paragrafo(texto) | Bloco::Citacao(texto) => vec![texto],
            Bloco::Codigo(s) => {
                let marcas = Marcas { codigo: true, ..Marcas::default() };
                vec![vec![Trecho::Texto { texto: s, marcas, link: None }]]
            }
            Bloco::Tabela(t) => t
                .linhas
                .into_iter()
                .flatten()
                .flat_map(|celula| celula.paragrafos)
                .collect(),
        }
    }
}

/// Chave de uma nota no mapa `Documento::notas`: `n` para rodapé, `e` para nota de fim.
pub(crate) fn chave_nota(fim: bool, id: &str) -> String {
    if fim {
        format!("e{id}")
    } else {
        format!("n{id}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texto(s: &str) -> Trecho {
        Trecho::Texto { texto: s.into(), marcas: Marcas::default(), link: None }
    }

    #[test]
    fn paragrafos_do_codigo_tem_marca_de_codigo() {
        let bloco = Bloco::Codigo("fn main() {}".into());
        let ps = bloco.paragrafos();
        assert_eq!(ps.len(), 1);
        match &ps[0][..] {
            [Trecho::Texto { texto, marcas, link: None }] => {
                assert_eq!(texto, "fn main() {}");
                assert!(marcas.codigo);
                assert!(!marcas.negrito);
            }
            outro => panic!("trecho inesperado: {outro:?}"),
        }
    }

    #[test]
    fn paragrafos_da_tabela_percorre_as_celulas_em_ordem() {
        let celula = |s: &str| Celula {
            paragrafos: vec![vec![texto(s)], vec![texto(&format!("{s}2"))]],
            colunas: 1,
            continua: false,
        };
        let tabela = Tabela { linhas: vec![vec![celula("a"), celula("b")], vec![celula("c")]] };
        let ps = Bloco::Tabela(tabela).paragrafos();
        let nomes: Vec<String> = ps
            .iter()
            .map(|p| match &p[0] {
                Trecho::Texto { texto, .. } => texto.clone(),
                outro => panic!("{outro:?}"),
            })
            .collect();
        assert_eq!(nomes, ["a", "a2", "b", "b2", "c", "c2"]);
    }

    #[test]
    fn chave_de_nota_distingue_rodape_de_fim() {
        assert_eq!(chave_nota(false, "2"), "n2");
        assert_eq!(chave_nota(true, "2"), "e2");
    }
}
