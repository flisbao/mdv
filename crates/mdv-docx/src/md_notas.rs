//! Numeração das notas de rodapé e fim e escrita das definições `[^N]: texto`.

use std::collections::HashMap;

use crate::md_inline::{inline, Modo};
use crate::modelo::Trecho;

/// Notas referenciadas até agora, na ordem da primeira aparição.
pub(crate) struct Notas {
    chaves: Vec<String>,
    // Índice chave -> número, para não varrer `chaves` a cada referência.
    numeros: HashMap<String, usize>,
}

impl Notas {
    /// Cria o controle sem nenhuma nota referenciada.
    pub(crate) fn novo() -> Self {
        Notas { chaves: Vec::new(), numeros: HashMap::new() }
    }

    /// Devolve o número (1, 2, 3...) da nota; chave repetida devolve o mesmo número.
    pub(crate) fn referenciar(&mut self, chave: &str) -> usize {
        if let Some(&numero) = self.numeros.get(chave) {
            return numero;
        }
        self.chaves.push(chave.to_string());
        self.numeros.insert(chave.to_string(), self.chaves.len());
        self.chaves.len()
    }

    /// Escreve as linhas `[^N]: texto` das notas referenciadas; vazio se não houve nenhuma.
    ///
    /// A numeração vem da ordem de referência, não da ordem das notas no Word. O texto da nota
    /// sai numa linha só (`Modo::Linha`) e sem `NotaRef`, que dentro de uma definição não faria
    /// sentido; uma chave sem texto (ou com texto vazio) ainda ganha linha, para a referência
    /// `[^N]` do corpo nunca ficar sem definição.
    pub(crate) fn definicoes(&self, textos: &HashMap<String, Vec<Trecho>>) -> String {
        let linhas: Vec<String> = self
            .chaves
            .iter()
            .enumerate()
            .map(|(i, chave)| format!("[^{}]: {}", i + 1, texto_da_nota(textos.get(chave))))
            .collect();
        linhas.join("\n")
    }
}

/// Texto de uma nota em uma linha; "(nota ausente)" quando não há texto.
fn texto_da_nota(trechos: Option<&Vec<Trecho>>) -> String {
    let sem_refs: Vec<Trecho> = trechos
        .into_iter()
        .flatten()
        .filter(|t| !matches!(t, Trecho::NotaRef(_)))
        .cloned()
        .collect();
    let texto = inline(&sem_refs, &mut Notas::novo(), Modo::Linha);
    if texto.is_empty() {
        "(nota ausente)".to_string()
    } else {
        texto
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modelo::Marcas;

    fn texto(s: &str) -> Vec<Trecho> {
        vec![Trecho::Texto { texto: s.into(), marcas: Marcas::default(), link: None }]
    }

    #[test]
    fn numera_por_ordem_de_aparicao() {
        let mut notas = Notas::novo();
        assert_eq!(notas.referenciar("n7"), 1);
        assert_eq!(notas.referenciar("e2"), 2);
        assert_eq!(notas.referenciar("n7"), 1);
    }

    #[test]
    fn definicoes_ordenadas_e_em_linha() {
        let mut notas = Notas::novo();
        notas.referenciar("n2");
        notas.referenciar("n1");
        let mut textos = HashMap::new();
        textos.insert("n1".to_string(), texto("dois"));
        textos.insert("n2".to_string(), texto("um"));
        assert_eq!(notas.definicoes(&textos), "[^1]: um\n[^2]: dois");
    }

    #[test]
    fn nota_sem_texto() {
        let mut notas = Notas::novo();
        notas.referenciar("n");
        assert_eq!(notas.definicoes(&HashMap::new()), "[^1]: (nota ausente)");
    }

    #[test]
    fn sem_notas_devolve_vazio() {
        assert_eq!(Notas::novo().definicoes(&HashMap::new()), "");
    }

    #[test]
    fn nota_com_marcas_e_quebra_fica_em_uma_linha() {
        let mut notas = Notas::novo();
        notas.referenciar("n1");
        let mut textos = HashMap::new();
        let negrito = Marcas { negrito: true, ..Marcas::default() };
        textos.insert(
            "n1".to_string(),
            vec![
                Trecho::Texto { texto: "a".into(), marcas: negrito, link: None },
                Trecho::Quebra,
                Trecho::NotaRef("n9".into()),
                Trecho::Texto { texto: "b".into(), marcas: Marcas::default(), link: None },
            ],
        );
        assert_eq!(notas.definicoes(&textos), "[^1]: **a** b");
    }
}
