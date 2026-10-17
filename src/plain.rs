//! Modo `--plain`: imprime o documento renderizado (ANSI) e a ajuda.

use std::io::{self, Write};

use ratatui::crossterm::terminal;

use crate::load::{image_store, Source};
use crate::doc::{render_doc, DocOpts};
use crate::{cli, render, Failure};

/// Largura do `--plain` quando o tamanho do terminal é desconhecido.
const FALLBACK_TERMINAL_WIDTH: usize = 80;

pub(crate) const HELP: &str = "\
mdv - visualizador de Markdown no terminal

USO:
    mdv [opções] <arquivo.md>
    cat arquivo.md | mdv
    mdv json [opções] <arquivo|->
    mdv csv|tsv [opções] <arquivo|->
    mdv <pasta>            (seletor: .md da pasta por data, filtro ao digitar)
    (o formato também é deduzido da extensão: .json, .jsonl, .csv, .tsv)

OPÇÕES:
    -p, --plain        imprime no terminal (sem TUI), como `cat` com cores
    -w, --width <N>    largura máxima do texto (padrão: 100)
        --remote       baixa imagens de http(s):// (nunca de rede privada)
        --graphics <G> imagens em alta resolução: auto (padrão; detecta Kitty/Ghostty), kitty ou off (só no modo interativo)
        --toc          imprime só o índice de títulos
        --stats        imprime palavras, blocos de código e tokens estimados, no total e por seção
        --follow       com stdin: acompanha o texto conforme chega
        --table <N>    abre direto no zoom da N-ésima tabela (só no modo interativo)
        --memory-buffer <T>  limite de memória (padrão: 512M; ex.: 256M, 2G; ou $MDV_MEMORY_BUFFER)
    -h, --help         mostra esta ajuda
    -V, --version      mostra a versão

OPÇÕES DE JSON (só valem para este formato):
        --path <p>     mostra só um trecho, ex.: .itens[0].nome
        --as <vista>   auto (padrão), text, table ou schema

OPÇÕES DE CSV/TSV (só valem para este formato):
        --head <N>     mostra só as N primeiras linhas (padrão: 1000)
        --cols <a,b>   mostra só estas colunas, pelo nome do cabeçalho
        --no-header    a primeira linha é dado, não cabeçalho
        --delimiter <c>  separador de um caractere (ou `tab`)

TECLAS (modo interativo):
    j / k, ↓ / ↑       rola uma linha
    espaço / b         página abaixo / acima
    d / u              meia página
    g / G              início / fim
    ] / [              próximo / título anterior
    /                  buscar (Enter confirma, Esc cancela)
    n / N              próxima / anterior ocorrência
    Tab / Shift-Tab    seleciona o próximo / anterior link; Enter segue (âncora, .md relativo ou URL http(s)/mailto com confirmação `y`), Esc cancela
    Backspace / H      volta ao documento anterior (pilha de até 100)
    m                  alterna entre renderizado e fonte crua
    w                  código sem quebra: h/l (ou ←/→) rolam 8 colunas, 0/$ início/fim; w de novo volta a quebrar
    v                  (JSON) alterna tabela / esquema / texto
    t                  zoom da tabela mais visível na tela (h/l colunas, n/p troca de tabela, Esc volta)
    i                  zoom do diagrama Mermaid mais visível na tela: n/p troca de diagrama, +/- zoom, h/j/k/l movem, espaço/b página, g/G topo/fim, 0 ajusta, Esc volta
    y / Y              copia o bloco de código da tela / o Markdown da seção (OSC 52, até 64 KiB)
    r                  recarrega o arquivo
    q, Esc             sai
";

/// Imprime o documento renderizado (com cores só se a saída for um terminal).
pub(crate) fn run_plain(md: &str, source: &Source, width: Option<usize>, color: bool, cli: &cli::Cli) -> Result<(), Failure> {
    let width = width.unwrap_or_else(|| {
        terminal::size()
            .map_or(FALLBACK_TERMINAL_WIDTH, |(cols, _)| usize::from(cols))
    });
    let images = image_store(source, cli, true);
    let out = render_doc(&DocOpts::from_cli(cli), md, width, &images);
    write_stdout(&render::to_ansi(&out.lines, color))
}

pub(crate) fn write_stdout(text: &str) -> Result<(), Failure> {
    write_tolerating_closed_pipe(io::stdout().lock(), text.as_bytes())
        .map_err(|e| Failure::Runtime(format!("não consegui escrever a saída: {e}")))
}

/// Escreve tudo em `out`. Se o leitor fechou o pipe (`mdv -p x | head`), a saída
/// foi simplesmente encerrada: isso não é erro.
pub(crate) fn write_tolerating_closed_pipe(mut out: impl Write, bytes: &[u8]) -> io::Result<()> {
    match out.write_all(bytes).and_then(|()| out.flush()) {
        Err(e) if e.kind() == io::ErrorKind::BrokenPipe => Ok(()),
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FailingWriter(io::ErrorKind);

    impl Write for FailingWriter {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            Err(self.0.into())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn pipe_quebrado_na_escrita_e_saida_normal() {
        assert!(write_tolerating_closed_pipe(FailingWriter(io::ErrorKind::BrokenPipe), b"x").is_ok());
    }

    #[test]
    fn outros_erros_de_escrita_continuam_erros() {
        let err = write_tolerating_closed_pipe(FailingWriter(io::ErrorKind::PermissionDenied), b"x");
        assert_eq!(err.unwrap_err().kind(), io::ErrorKind::PermissionDenied);
    }
}
