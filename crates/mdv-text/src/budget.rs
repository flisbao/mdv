//! Orçamento de trabalho: o ponto onde o chamador (o `mdv`) limita a memória das bibliotecas puras.

/// Orçamento de trabalho consultado pelos laços que crescem com a entrada.
/// `Err(mensagem)` manda parar; a mensagem sobe como erro da conversão.
pub trait Budget {
    /// Confirma que ainda há orçamento.
    fn check(&self) -> Result<(), String>;
}

/// Orçamento sem limite, para testes e uso interno controlado.
pub struct Unlimited;

impl Budget for Unlimited {
    fn check(&self) -> Result<(), String> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sem_limite_nunca_recusa() {
        assert_eq!(Unlimited.check(), Ok(()));
    }
}
