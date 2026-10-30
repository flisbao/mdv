//! Classificação do parágrafo: decide se vira título, item de lista, código, citação ou texto.
//! Recebe propriedades e trechos já lidos; não conhece XML.

use crate::contexto::Contexto;
use crate::modelo::{Bloco, Trecho};
use crate::numeracao::Marca;
use crate::propriedades::Propriedades;
use crate::MAX_NIVEL_LISTA;

/// Transforma o parágrafo lido em bloco, ou `None` se não há nada visível.
/// A ordem é a de prioridade: vazio, título, lista, código, citação e, por fim, parágrafo.
/// Título vence lista porque o Word numera títulos (`1.`, `1.1.`) sem que eles virem itens.
pub(crate) fn classificar(props: &Propriedades, trechos: Vec<Trecho>, ctx: &mut Contexto) -> Option<Bloco> {
    if !tem_conteudo(&trechos) {
        return None;
    }
    let estilo = props.estilo.as_deref().map(|id| ctx.estilos.de(id)).unwrap_or_default();
    if let Some(nivel) = props.nivel.or(estilo.titulo) {
        return Some(Bloco::Titulo { nivel, texto: trechos });
    }
    // `numPr` direto vence o estilo, até quando é `numId` 0: aí o autor tirou o parágrafo da
    // lista que o estilo daria, e cair no estilo traria a lista de volta.
    let lista = match props.num {
        Some((0, _)) => None,
        Some(direto) => Some(direto),
        None => estilo.numeracao,
    };
    if let Some((num_id, nivel)) = lista {
        let nivel = nivel.min(MAX_NIVEL_LISTA);
        let numerada = ctx.numeracao.marca(num_id, nivel) == Marca::Numerada;
        let numero = if numerada { ctx.contador.proximo(ctx.numeracao, num_id, nivel) } else { 0 };
        return Some(Bloco::Item { nivel, numerada, numero, texto: trechos });
    }
    if estilo.codigo || todo_em_codigo(&trechos) {
        return Some(Bloco::Codigo(juntar_codigo(&trechos)));
    }
    if estilo.citacao {
        return Some(Bloco::Citacao(trechos));
    }
    Some(Bloco::Paragrafo(trechos))
}

/// Há algo a mostrar: texto que não seja só espaço, imagem ou referência de nota.
fn tem_conteudo(trechos: &[Trecho]) -> bool {
    trechos.iter().any(|t| match t {
        Trecho::Texto { texto, .. } => !texto.trim().is_empty(),
        Trecho::Imagem(_) | Trecho::NotaRef(_) => true,
        Trecho::Quebra => false,
    })
}

/// Todos os trechos de texto estão em fonte de código (e há pelo menos um).
fn todo_em_codigo(trechos: &[Trecho]) -> bool {
    let mut textos = trechos.iter().filter_map(|t| match t {
        Trecho::Texto { marcas, .. } => Some(marcas.codigo),
        _ => None,
    });
    let primeiro = textos.next();
    primeiro == Some(true) && textos.all(|c| c)
}

/// Texto do bloco de código: `Quebra` vira `\n`; imagem e nota não cabem em código e somem.
fn juntar_codigo(trechos: &[Trecho]) -> String {
    let mut saida = String::new();
    for t in trechos {
        match t {
            Trecho::Texto { texto, .. } => saida.push_str(texto),
            Trecho::Quebra => saida.push('\n'),
            Trecho::NotaRef(_) | Trecho::Imagem(_) => {}
        }
    }
    saida
}
