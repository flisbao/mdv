//! Contexto de uma leitura: contadores, orçamento e as tabelas de links e títulos.
//! Todo laço que percorre nós ou linhas passa por `tick` ou `linhas`.

use crate::style::Cascade;
use crate::Budget;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;

/// Tamanho máximo, em caracteres, de um `href` guardado.
const MAX_HREF: usize = 2048;

/// Estado compartilhado pelo layout de um documento.
pub struct Ctx<'a> {
    pub cascade: &'a Cascade<'a>,
    pub budget: &'a dyn Budget,
    /// URLs dos links, na ordem em que aparecem.
    pub links: RefCell<Vec<String>>,
    /// Títulos: (linha dentro do bloco raiz, nível de 1 a 6, texto).
    pub headings: RefCell<Vec<(usize, u8, String)>>,
    /// Largura, em colunas, do bloco que contém o texto em linha (limita as barras).
    pub largura: Cell<usize>,
    /// Elementos visitados até agora.
    pub elements: Cell<usize>,
    /// Níveis de flex aninhados em layout agora (acima de `NIVEL_MEDIDO`, o flex não mede os itens).
    pub flex_nivel: Cell<usize>,
    /// Linhas de texto produzidas até agora.
    pub lines: Cell<usize>,
    indices: RefCell<HashMap<String, u32>>,
}

impl<'a> Ctx<'a> {
    pub fn new(cascade: &'a Cascade<'a>, budget: &'a dyn Budget) -> Self {
        Ctx {
            cascade,
            budget,
            links: RefCell::new(Vec::new()),
            headings: RefCell::new(Vec::new()),
            largura: Cell::new(80),
            elements: Cell::new(0),
            flex_nivel: Cell::new(0),
            lines: Cell::new(0),
            indices: RefCell::new(HashMap::new()),
        }
    }

    /// Conta um elemento visitado; recusa páginas com elementos demais e consulta o orçamento.
    pub fn tick(&self) -> Result<(), String> {
        let n = self.elements.get().saturating_add(1);
        self.elements.set(n);
        if n > self.cascade.lim.max_elements {
            return Err("página complexa demais".to_string());
        }
        self.budget.check()
    }

    /// Conta `n` linhas de texto; recusa páginas com linhas demais.
    pub fn linhas(&self, n: usize) -> Result<(), String> {
        let total = self.lines.get().saturating_add(n);
        self.lines.set(total);
        if total > self.cascade.lim.max_lines {
            return Err("página longa demais".to_string());
        }
        self.budget.check()
    }

    /// Registra um link e devolve seu índice. Remove controles, corta em 2 048 caracteres
    /// e reaproveita o índice de um `href` igual já visto.
    pub fn add_link(&self, href: &str) -> u32 {
        let limpo: String = href.chars().filter(|c| !c.is_control()).take(MAX_HREF).collect();
        if let Some(i) = self.indices.borrow().get(&limpo) {
            return *i;
        }
        let mut links = self.links.borrow_mut();
        let i = u32::try_from(links.len()).unwrap_or(u32::MAX);
        self.indices.borrow_mut().insert(limpo.clone(), i);
        links.push(limpo);
        i
    }
}
