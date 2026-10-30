//! Numeração de listas do `word/numbering.xml`: diz se um nível é marcador ou numerado e
//! em qual número cada item começa. `Contador` soma os itens por `numId`.

use std::collections::HashMap;

use crate::xml_leitor::{Evento, Leitor, Tag};
use crate::Budget;
use crate::MAX_NIVEL_LISTA;

/// Quantidade de níveis de uma lista (0 até `MAX_NIVEL_LISTA`).
const NIVEIS: usize = MAX_NIVEL_LISTA as usize + 1;

/// Índice do nível dentro dos arrays; nível acima do teto usa o último (o Word só tem 9).
fn indice(nivel: u8) -> usize {
    usize::from(nivel.min(MAX_NIVEL_LISTA))
}

/// Nível de um `w:ilvl` ou `w:lvl`; fora de 0..=`MAX_NIVEL_LISTA` vira `None` e é ignorado.
fn ler_nivel(tag: &Tag) -> Option<u8> {
    tag.attr("ilvl").and_then(|v| v.parse::<u8>().ok()).filter(|&n| n <= MAX_NIVEL_LISTA)
}

/// Como o item de um nível é marcado: marcador (`•`) ou número.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Marca {
    Marcador,
    Numerada,
}

/// Um nível de um `abstractNum`: a marca e o `start` declarado, se houver.
#[derive(Clone, Copy, Default)]
struct Nivel {
    marca: Option<Marca>,
    start: Option<u32>,
}

/// Um `w:num`: aponta para um `abstractNum` e pode trocar o início de cada nível.
#[derive(Clone, Default)]
struct Num {
    abstrato: Option<u32>,
    sobrepostos: [Option<u32>; NIVEIS],
}

/// Definições de listas do documento, já ligadas: `numId` → `abstractNum` → níveis.
pub(crate) struct Numeracao {
    abstratos: HashMap<u32, [Nivel; NIVEIS]>,
    nums: HashMap<u32, Num>,
}

impl Numeracao {
    /// Sem nenhuma lista: todo `numId` cai no marcador.
    pub(crate) fn vazia() -> Self {
        Numeracao { abstratos: HashMap::new(), nums: HashMap::new() }
    }

    /// Lê `numbering.xml`; lista desconhecida não é erro.
    pub(crate) fn ler(xml: &[u8], budget: &dyn Budget) -> Result<Self, String> {
        let mut leitor = Leitor::novo(xml, budget);
        let mut numeracao = Numeracao::vazia();
        // Consome a raiz `w:numbering` antes do laço: o laço trata os filhos dela, não a raiz.
        if leitor.ate("numbering")?.is_none() {
            return Ok(numeracao);
        }
        while let Some(evento) = leitor.proximo()? {
            match evento {
                Evento::Abre(tag) if tag.nome == "abstractNum" => {
                    ler_abstrato(&mut leitor, &tag, &mut numeracao)?
                }
                Evento::Abre(tag) if tag.nome == "num" => ler_num(&mut leitor, &tag, &mut numeracao)?,
                // Outros filhos da raiz (`numPicBullet`, ...) não interessam e são pulados inteiros.
                Evento::Abre(_) => leitor.pular()?,
                Evento::Fecha(nome) if nome == "numbering" => break,
                Evento::Fecha(_) | Evento::Texto(_) => {}
            }
        }
        Ok(numeracao)
    }

    /// Marca do nível; `numId` desconhecido ou nível sem `numFmt` vira marcador.
    pub(crate) fn marca(&self, num_id: u32, nivel: u8) -> Marca {
        self.nivel_do(num_id, indice(nivel))
            .and_then(|n| n.marca)
            .unwrap_or(Marca::Marcador)
    }

    /// Número em que o nível começa; padrão 1.
    /// Precedência: `startOverride` do `w:num` vence o `start` do nível, que vence o padrão 1.
    /// Uma lista que reinicia (ex.: "continuar de onde parou" no Word) depende dessa ordem.
    pub(crate) fn inicio(&self, num_id: u32, nivel: u8) -> u32 {
        let idx = indice(nivel);
        self.nums
            .get(&num_id)
            .and_then(|n| n.sobrepostos[idx])
            .or_else(|| self.nivel_do(num_id, idx).and_then(|n| n.start))
            .unwrap_or(1)
    }

    /// Nível de um `numId` já ligado ao seu `abstractNum`; `None` se a cadeia estiver quebrada.
    fn nivel_do(&self, num_id: u32, idx: usize) -> Option<&Nivel> {
        let abstrato = self.nums.get(&num_id)?.abstrato?;
        self.abstratos.get(&abstrato).map(|niveis| &niveis[idx])
    }
}

/// Lê um `w:abstractNum` até o fecho. O id é atributo aqui, mas `w:num` o repete como elemento.
fn ler_abstrato(leitor: &mut Leitor, tag: &Tag, numeracao: &mut Numeracao) -> Result<(), String> {
    let id = tag.attr("abstractNumId").and_then(|v| v.parse::<u32>().ok());
    let mut niveis = [Nivel::default(); NIVEIS];
    loop {
        match leitor.proximo()? {
            None => return Err("fim inesperado do documento".into()),
            Some(Evento::Fecha(n)) if n == "abstractNum" => break,
            Some(Evento::Abre(filho)) if filho.nome == "lvl" => {
                let nivel = ler_lvl(leitor)?;
                if let Some(n) = ler_nivel(&filho) {
                    niveis[usize::from(n)] = nivel;
                }
            }
            Some(Evento::Abre(_)) => leitor.pular()?,
            Some(_) => {}
        }
    }
    // Sem `abstractNumId` válido o bloco não tem como ser referenciado; é lido e descartado.
    if let Some(id) = id {
        numeracao.abstratos.insert(id, niveis);
    }
    Ok(())
}

/// Lê um `w:lvl` até o fecho: `start` e `numFmt`; o resto (`pPr`, `rPr`...) é pulado.
fn ler_lvl(leitor: &mut Leitor) -> Result<Nivel, String> {
    let mut nivel = Nivel::default();
    loop {
        match leitor.proximo()? {
            None => return Err("fim inesperado do documento".into()),
            Some(Evento::Fecha(n)) if n == "lvl" => break,
            Some(Evento::Abre(tag)) => match tag.nome.as_str() {
                "start" => {
                    nivel.start = tag.val().and_then(|v| v.parse::<u32>().ok());
                    leitor.pular()?;
                }
                // Só `bullet` (e `none`, que não mostra nada) é marcador; qualquer formato de número é numerado.
                "numFmt" => {
                    nivel.marca = tag.val().map(|v| match v {
                        "bullet" | "none" => Marca::Marcador,
                        _ => Marca::Numerada,
                    });
                    leitor.pular()?;
                }
                _ => leitor.pular()?,
            },
            Some(_) => {}
        }
    }
    Ok(nivel)
}

/// Lê um `w:num` até o fecho: `abstractNumId` e os `lvlOverride`. O id só é guardado se for válido.
fn ler_num(leitor: &mut Leitor, tag: &Tag, numeracao: &mut Numeracao) -> Result<(), String> {
    let id = tag.attr("numId").and_then(|v| v.parse::<u32>().ok());
    let mut num = Num::default();
    loop {
        match leitor.proximo()? {
            None => return Err("fim inesperado do documento".into()),
            Some(Evento::Fecha(n)) if n == "num" => break,
            Some(Evento::Abre(filho)) => match filho.nome.as_str() {
                "abstractNumId" => {
                    num.abstrato = filho.val().and_then(|v| v.parse::<u32>().ok());
                    leitor.pular()?;
                }
                "lvlOverride" => ler_override(leitor, &filho, &mut num)?,
                _ => leitor.pular()?,
            },
            Some(_) => {}
        }
    }
    if let Some(id) = id {
        numeracao.nums.insert(id, num);
    }
    Ok(())
}

/// Lê um `w:lvlOverride`: só o `startOverride` importa; o `lvl` completo que ele pode trazer é pulado.
fn ler_override(leitor: &mut Leitor, tag: &Tag, num: &mut Num) -> Result<(), String> {
    let nivel = ler_nivel(tag);
    loop {
        match leitor.proximo()? {
            None => return Err("fim inesperado do documento".into()),
            Some(Evento::Fecha(n)) if n == "lvlOverride" => break,
            Some(Evento::Abre(filho)) if filho.nome == "startOverride" => {
                let valor = filho.val().and_then(|v| v.parse::<u32>().ok());
                if let (Some(n), Some(v)) = (nivel, valor) {
                    num.sobrepostos[usize::from(n)] = Some(v);
                }
                leitor.pular()?;
            }
            Some(Evento::Abre(_)) => leitor.pular()?,
            Some(_) => {}
        }
    }
    Ok(())
}

/// Contadores de itens de cada lista, por `numId`.
pub(crate) struct Contador {
    contagens: HashMap<u32, [u32; NIVEIS]>,
    /// Dentro de célula ou nota o item vira texto sem marcador, então não conta.
    parado: bool,
}

impl Contador {
    /// Sem nenhum item contado.
    pub(crate) fn novo() -> Self {
        Contador { contagens: HashMap::new(), parado: false }
    }

    /// Para ou retoma a contagem e devolve o estado anterior, para restaurá-lo depois.
    /// Um item numerado dentro de célula ou de nota sai sem marcador; se contasse, deslocaria
    /// a numeração da lista do corpo que usa o mesmo `numId`.
    pub(crate) fn parar(&mut self, parar: bool) -> bool {
        std::mem::replace(&mut self.parado, parar)
    }

    /// Número do próximo item deste nível e `numId`.
    /// Item de nível mais raso zera os níveis mais fundos: no Word, a sub-lista recomeça
    /// do `inicio` a cada item do pai. Contadores são por `numId` (limitação conhecida):
    /// `numId`s diferentes que compartilham um `abstractNum` não continuam a contagem um do outro.
    pub(crate) fn proximo(&mut self, num: &Numeracao, num_id: u32, nivel: u8) -> u32 {
        if self.parado {
            return 0;
        }
        let nivel = nivel.min(MAX_NIVEL_LISTA);
        let idx = indice(nivel);
        let conta = self.contagens.entry(num_id).or_insert([0; NIVEIS]);
        // 0 marca "lista ainda não começou neste nível"; senão guarda o próximo número a sair.
        let numero = if conta[idx] == 0 { num.inicio(num_id, nivel) } else { conta[idx] };
        conta[idx] = numero.saturating_add(1);
        for fundo in &mut conta[idx + 1..] {
            *fundo = 0;
        }
        numero
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::NS;
    use crate::Unlimited;

    /// Monta `w:numbering` com estes filhos e lê.
    fn ler_xml(corpo: &str) -> Numeracao {
        let xml = format!("<w:numbering {NS}>{corpo}</w:numbering>");
        Numeracao::ler(xml.as_bytes(), &Unlimited).unwrap()
    }

    /// Um `abstractNum` 0 com os formatos dados nos níveis 0, 1, 2 e 3 (sem `start`).
    fn abstrato_com(formatos: [&str; 4]) -> String {
        let niveis: String = formatos
            .iter()
            .enumerate()
            .map(|(i, f)| {
                format!(r#"<w:lvl w:ilvl="{i}"><w:numFmt w:val="{f}"/></w:lvl>"#)
            })
            .collect();
        format!(
            r#"<w:abstractNum w:abstractNumId="0">{niveis}</w:abstractNum><w:num w:numId="1"><w:abstractNumId w:val="0"/></w:num>"#
        )
    }

    #[test]
    fn bullet_e_decimal() {
        let n = ler_xml(&abstrato_com(["bullet", "decimal", "decimal", "decimal"]));
        assert_eq!(n.marca(1, 0), Marca::Marcador);
        assert_eq!(n.marca(1, 1), Marca::Numerada);
    }

    #[test]
    fn letras_e_romanos_sao_numeradas() {
        let n = ler_xml(&abstrato_com(["lowerLetter", "upperRoman", "lowerRoman", "upperLetter"]));
        assert_eq!(n.marca(1, 0), Marca::Numerada);
        assert_eq!(n.marca(1, 1), Marca::Numerada);
        assert_eq!(n.marca(1, 2), Marca::Numerada);
        assert_eq!(n.marca(1, 3), Marca::Numerada);
    }

    #[test]
    fn formato_none_e_marcador() {
        let n = ler_xml(&abstrato_com(["none", "decimal", "decimal", "decimal"]));
        assert_eq!(n.marca(1, 0), Marca::Marcador);
    }

    #[test]
    fn numero_desconhecido_e_marcador() {
        let n = ler_xml(&abstrato_com(["decimal", "decimal", "decimal", "decimal"]));
        assert_eq!(n.marca(99, 0), Marca::Marcador);
        assert_eq!(n.inicio(99, 0), 1);
    }

    #[test]
    fn contador_sequencia_e_reinicio_dos_niveis() {
        let n = ler_xml(&abstrato_com(["decimal", "decimal", "decimal", "decimal"]));
        let mut c = Contador::novo();
        assert_eq!(c.proximo(&n, 1, 0), 1);
        assert_eq!(c.proximo(&n, 1, 0), 2);
        assert_eq!(c.proximo(&n, 1, 1), 1);
        assert_eq!(c.proximo(&n, 1, 1), 2);
        assert_eq!(c.proximo(&n, 1, 0), 3);
        assert_eq!(c.proximo(&n, 1, 1), 1);
    }

    #[test]
    fn start_do_nivel() {
        let n = ler_xml(
            r#"<w:abstractNum w:abstractNumId="0"><w:lvl w:ilvl="0"><w:start w:val="5"/><w:numFmt w:val="decimal"/></w:lvl></w:abstractNum><w:num w:numId="1"><w:abstractNumId w:val="0"/></w:num>"#,
        );
        assert_eq!(n.inicio(1, 0), 5);
        let mut c = Contador::novo();
        assert_eq!(c.proximo(&n, 1, 0), 5);
        assert_eq!(c.proximo(&n, 1, 0), 6);
    }

    #[test]
    fn start_override_do_num() {
        let n = ler_xml(&format!(
            r#"{}<w:num w:numId="2"><w:abstractNumId w:val="0"/><w:lvlOverride w:ilvl="0"><w:startOverride w:val="7"/></w:lvlOverride></w:num>"#,
            abstrato_com(["decimal", "decimal", "decimal", "decimal"])
        ));
        assert_eq!(n.inicio(2, 0), 7);
        assert_eq!(n.inicio(1, 0), 1);
    }

    #[test]
    fn listas_diferentes_tem_contadores_proprios() {
        let n = ler_xml(&format!(
            r#"{}<w:num w:numId="2"><w:abstractNumId w:val="0"/></w:num>"#,
            abstrato_com(["decimal", "decimal", "decimal", "decimal"])
        ));
        let mut c = Contador::novo();
        assert_eq!(c.proximo(&n, 1, 0), 1);
        assert_eq!(c.proximo(&n, 2, 0), 1);
    }

    #[test]
    fn valores_invalidos_sao_ignorados() {
        let n = ler_xml(
            r#"<w:abstractNum w:abstractNumId="x"><w:lvl w:ilvl="0"><w:start w:val="abc"/></w:lvl></w:abstractNum>
               <w:abstractNum w:abstractNumId="0"><w:lvl w:ilvl="99"><w:numFmt w:val="bullet"/></w:lvl></w:abstractNum>
               <w:num w:numId="1"><w:abstractNumId w:val="0"/><w:lvlOverride w:ilvl="0"><w:startOverride w:val="-3"/></w:lvlOverride></w:num>"#,
        );
        assert_eq!(n.marca(1, 0), Marca::Marcador);
        assert_eq!(n.inicio(1, 0), 1);
    }

    #[test]
    fn nivel_acima_do_teto_vira_o_maximo() {
        let n = ler_xml(&abstrato_com(["decimal", "decimal", "decimal", "decimal"]));
        let mut c = Contador::novo();
        assert_eq!(c.proximo(&n, 1, 200), 1);
        assert_eq!(c.proximo(&n, 1, MAX_NIVEL_LISTA), 2);
        assert_eq!(n.marca(1, 200), n.marca(1, MAX_NIVEL_LISTA));
    }
}
