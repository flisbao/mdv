//! `Failure`: a falha que encerra o programa, com o código de saída e a mensagem do stderr.

use std::process::ExitCode;

use crate::render;

/// Falha que encerra o programa; o código de saída depende do tipo.
pub(crate) enum Failure {
    /// Uso incorreto da linha de comando (código 2).
    Usage(String),
    /// Erro ao ler a entrada ou usar o terminal (código 1).
    Runtime(String),
}

impl Failure {
    pub(crate) fn exit_code(&self) -> ExitCode {
        match self {
            Failure::Usage(_) => ExitCode::from(2),
            Failure::Runtime(_) => ExitCode::from(1),
        }
    }

    /// Escreve a mensagem no stderr; o texto passa por `sanitize` (pode vir do arquivo).
    pub(crate) fn report(&self) {
        match self {
            Failure::Usage(msg) => eprintln!("mdv: {}\nUse `mdv --help` para ver as opções.", render::sanitize(msg)),
            Failure::Runtime(msg) => eprintln!("mdv: {}", render::sanitize(msg)),
        }
    }
}
