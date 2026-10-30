//! Numeração das notas de rodapé e fim e escrita das definições `[^N]: texto`.

use std::collections::HashMap;

use crate::escape;
use crate::limite::Limite;
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
    pub(crate) fn definicoes(
        &self,
        textos: &HashMap<String, Vec<Trecho>>,
        limite: &mut Limite,
    ) -> Result<String, String> {
        let mut linhas: Vec<String> = Vec::with_capacity(self.chaves.len());
        for (i, chave) in self.chaves.iter().enumerate() {
            let linha = format!("[^{}]: {}", i + 1, texto_nota(textos.get(chave)));
            limite.gastar(linha.len() + 1)?;
            linhas.push(linha);
        }
        Ok(linhas.join("\n"))
    }
}

/// Texto de uma nota em uma linha; "(nota ausente)" quando não há texto.
fn texto_nota(trechos: Option<&Vec<Trecho>>) -> String {
    let sem_refs: Vec<Trecho> = trechos
        .into_iter()
        .flatten()
        .filter(|t| !matches!(t, Trecho::NotaRef(_)))
        .cloned()
        .collect();
    // O texto vem logo depois de `[^N]: `, onde `# ` ou `> ` abririam um bloco dentro da nota.
    let texto = escape::inicio_de_linha(&inline(&sem_refs, &mut Notas::novo(), Modo::Linha));
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
    use crate::Unlimited;

    fn ilimitado() -> Limite<'static> {
        Limite::novo(&Unlimited, usize::MAX)
    }

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
        assert_eq!(notas.definicoes(&textos, &mut ilimitado()).unwrap(), "[^1]: um\n[^2]: dois");
    }

    #[test]
    fn nota_sem_texto() {
        let mut notas = Notas::novo();
        notas.referenciar("n");
        assert_eq!(notas.definicoes(&HashMap::new(), &mut ilimitado()).unwrap(), "[^1]: (nota ausente)");
    }

    #[test]
    fn texto_de_nota_que_parece_bloco_e_escapado() {
        let mut notas = Notas::novo();
        notas.referenciar("n1");
        let mut textos = HashMap::new();
        textos.insert("n1".to_string(), texto("# Segredo"));
        assert_eq!(notas.definicoes(&textos, &mut ilimitado()).unwrap(), "[^1]: \\# Segredo");
    }

    #[test]
    fn notas_acima_do_teto_sao_recusadas() {
        let mut notas = Notas::novo();
        let mut textos = HashMap::new();
        for i in 0..100 {
            notas.referenciar(&format!("n{i}"));
            textos.insert(format!("n{i}"), texto("conteúdo"));
        }
        let mut limite = Limite::novo(&Unlimited, 100);
        let erro = notas.definicoes(&textos, &mut limite).expect_err("erro");
        assert!(erro.contains("grande demais"), "{erro}");
    }

    #[test]
    fn sem_notas_devolve_vazio() {
        assert_eq!(Notas::novo().definicoes(&HashMap::new(), &mut ilimitado()).unwrap(), "");
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
        assert_eq!(notas.definicoes(&textos, &mut ilimitado()).unwrap(), "[^1]: **a** b");
    }
}
