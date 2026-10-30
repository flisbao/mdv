//! Teto de bytes da saída de um grupo, descontado à medida que o texto é produzido.

use crate::Budget;

/// Erro de saída acima do teto (também usado na soma final do documento).
pub(crate) const ERRO_GRANDE: &str = "documento convertido grande demais";

/// Bytes de saída que ainda podem ser produzidos, junto com o orçamento de trabalho.
pub(crate) struct Limite<'a> {
    budget: &'a dyn Budget,
    restante: usize,
}

impl<'a> Limite<'a> {
    /// Começa com `teto` bytes disponíveis.
    pub(crate) fn novo(budget: &'a dyn Budget, teto: usize) -> Self {
        Limite { budget, restante: teto }
    }

    /// Confere o orçamento e desconta `bytes`; erra ao passar do teto.
    ///
    /// Lista, citação, tabela e notas montam centenas de milhares de pedaços num grupo só, e
    /// uma célula `gridSpan` ou uma URL repetida faz a saída ser muito maior que a entrada.
    /// Conferir só depois do grupo pronto já teria gasto a memória; por isso cada pedaço paga
    /// ao ser produzido e o grupo para no primeiro que estourar.
    pub(crate) fn gastar(&mut self, bytes: usize) -> Result<(), String> {
        self.budget.check()?;
        self.restante = self.restante.checked_sub(bytes).ok_or_else(|| ERRO_GRANDE.to_string())?;
        Ok(())
    }
}
