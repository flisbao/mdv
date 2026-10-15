# F7.9 — Limite de memória (`--memory-buffer`): Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fazer o `mdv` limitar o próprio uso de memória (padrão 512 MiB), parar com uma mensagem acionável ao estourar e permitir aumentar o limite com `--memory-buffer` / `MDV_MEMORY_BUFFER`, avisando quando o valor pedido passa da memória livre do computador.

**Architecture:** Um módulo novo `src/mem.rs` traz um alocador global que conta os bytes vivos, o limite (atômico), os pontos de checagem (`check`, `exceeded`, `check_input`) e funções puras de tamanho e mensagem. Os laços pesados de `render.rs`, `data.rs` e `main.rs` chamam a checagem e, ao estourar, devolvem `error_rendered(msg)`, descartando o que estavam montando. O alocador também recusa alocações acima de `limite × 3/2 + 16 MiB`, como rede de segurança.

**Tech Stack:** Rust 2021, `std::alloc::GlobalAlloc`, `std::sync::atomic`. Nenhuma dependência nova.

**Spec:** `docs/superpowers/specs/2026-10-15-fase-7-limite-de-memoria-design.md`; item F7.9 do `PLANO.md`.

## Global Constraints

- Mensagens de erro, ajuda e README em português, com acentuação correta.
- Esta fase não acrescenta dependência nem subprocesso. Leitura de `/proc/meminfo` e de variável de ambiente só em `src/mem.rs` e `src/main.rs`; `render.rs` e `data.rs` só chamam `mem::check()`/`mem::exceeded()` (um teste de contador, sem I/O).
- `DEFAULT_LIMIT = 512 MiB`; `MIN_LIMIT = 32 MiB`; teto duro do alocador `limite + limite/2 + 16 MiB` (saturante).
- Unidades de tamanho sempre em múltiplos de 1024 (`K/KB/KiB`, `M/MB/MiB`, `G/GB/GiB`, sem distinção de caixa; sem sufixo = bytes).
- Valor de `--memory-buffer` inválido, zero, com overflow ou abaixo de 32 MiB é **erro de uso** (código 2); o mesmo vale para `MDV_MEMORY_BUFFER` (só quando a flag não foi dada).
- Nenhum teste unitário altera o limite global (`mem::set_limit`): os testes rodam em paralelo e compartilham o contador. O estouro real é testado executando o binário (`tests/cli.rs`).
- Regra do `CLAUDE.md`: nenhum teste aloca memória sem teto. Rode a suíte sempre assim: `(ulimit -v 6000000; cargo test 2>&1 | tail -20)`.
- O estouro nunca imprime caracteres de controle: a mensagem passa por `error_rendered` (que já sanitiza).
- Código `unsafe` só no `impl GlobalAlloc`, delegando ao `System`.
- `cargo clippy --all-targets` sem avisos novos; `cargo test` verde ao fim de cada tarefa.

## Review Focus

- Arquivo pequeno em disco que expande muito na renderização (4 MiB de parágrafos de uma letra): a renderização para, sai a mensagem, o código de saída é 0 e o processo não aborta. Teste na Task 3.
- Arquivo cujo tamanho × 4 já passa do limite: recusado antes de ler, sem alocar. Teste na Task 3.
- `--memory-buffer` com `0`, `-1`, `1.5G`, `abc`, `999999999999G` (overflow) e `16M` (abaixo do mínimo): erro de uso, nunca pânico. Testes na Task 1 e na Task 2.
- `MDV_MEMORY_BUFFER` inválido sem a flag: erro de uso que cita a variável; com a flag válida, a variável é ignorada. Testes na Task 2.
- Memória livre desconhecida (macOS/Windows) ou `/proc/meminfo` ilegível: as mensagens saem na forma sem número, sem pânico. Testes na Task 1.
- Limite maior que a memória livre: aviso no stderr (`--plain`) ou na barra (TUI), sem bloquear. Testes na Task 1 e na Task 2.
- `--follow` com stdin crescendo além do limite: para de acumular em vez de crescer sem fim. Teste na Task 3.
- Contador do alocador nunca fica "negativo" (underflow): um erro de contagem faria toda checagem falhar e a suíte inteira quebrar de forma óbvia; o teste do contador (Task 1) pega o caso simples.

---

## File Structure

| Arquivo | Responsabilidade nesta fase |
|---|---|
| `src/mem.rs` (novo) | alocador contador, limite, `check`/`exceeded`/`check_input`, `parse_size`/`resolve`, `describe`/`fmt_size`/`flag_size`, `available_bytes`/`init`, aviso |
| `src/main.rs` | `mod mem`, `#[global_allocator]`, resolver e aplicar o limite, aviso, `HELP`, checagem em `read_markdown_file`, `drain_stream` |
| `src/cli.rs` | flag `--memory-buffer <TAMANHO>` |
| `src/render.rs` | checagem por evento em `render_with`, nos laços de `layout_table` e `render_code`, erro em `render_code_only`/`render_table_only` |
| `src/data.rs` | checagem depois do parse, por item e por registro |
| `tests/cli.rs` | estouro real, flag, variável de ambiente, arquivo grande demais |
| `README.md`, `CLAUDE.md`, `PLANO.md`, spec | documentação |

## Pontos do código atual que o plano usa

- `src/main.rs`: `run_cli()` (lê `cli::parse`, decide `interactive`, cria `App`), `Failure::Usage`/`Failure::Runtime`, `read_markdown_file(path)` (usa `fs::metadata` e `read_limited` com `MAX_INPUT_BYTES = 16 MiB`), `App::drain_stream`, `App.notice: Option<String>` (aviso na barra, some na próxima tecla), `HELP`, helper de teste `test_app(source, md)`.
- `src/cli.rs`: `Cli`, `parse`, helper de teste `run(&[..])`.
- `src/render.rs`: `render_with` (laço `for ev in Parser::new_ext(...)`), `render_code_only`, `render_table_only`, `error_rendered`, `layout_table` (laço `for (ri, row) in t.rows.iter().enumerate()`), `Renderer::render_code` (laço `for line in LinesWithEndings::from(&block.buf)`).
- `src/data.rs`: `render_json`, `render_table_data` (laço `for rec in rdr.records()`), `records_to_rows`, `flatten`/`KeyTree`, `error_rendered` importado de `crate::render`.
- O README tem um bloco `<!-- mdv:help:start -->` que copia o `HELP` à mão; não há teste que compare os dois.

---

### Task 1: Módulo `mem` (alocador contador, limite, tamanhos, mensagens)

**Files:**
- Create: `src/mem.rs`
- Modify: `src/main.rs` (`mod mem;` e `#[global_allocator]`)
- Test: `src/mem.rs` (módulo `tests`)

**Interfaces:**
- Consumes: nada.
- Produces (usadas pelas Tasks 2–3):
  - `pub const DEFAULT_LIMIT: usize`, `pub const MIN_LIMIT: usize`
  - `pub struct CountingAlloc;` (`unsafe impl GlobalAlloc`)
  - `pub fn used() -> usize`, `pub fn limit() -> usize`, `pub fn set_limit(bytes: usize)`
  - `pub const fn hard_cap(limit: usize) -> usize`
  - `pub struct MemoryLimit { pub used: usize, pub limit: usize }` com `Display` e `Debug`, `Clone`, `PartialEq`
  - `pub fn check() -> Result<(), MemoryLimit>`, `pub fn exceeded() -> bool`, `pub fn check_against(used: usize, limit: usize) -> Result<(), MemoryLimit>`, `pub fn check_input(len: u64) -> Result<(), MemoryLimit>`
  - `pub fn parse_size(s: &str) -> Result<usize, String>`, `pub fn resolve(flag: Option<usize>, env: Option<&str>) -> Result<usize, String>`
  - `pub fn fmt_size(bytes: u64) -> String`, `pub fn flag_size(bytes: usize) -> String`, `pub fn describe(limit: usize, available: Option<u64>) -> String`, `pub fn warn_if_above(limit: usize, available: Option<u64>) -> Option<String>`
  - `pub fn parse_meminfo(text: &str) -> Option<u64>`, `pub fn available_bytes() -> Option<u64>`, `pub fn init()`, `pub fn available() -> Option<u64>`

- [ ] **Step 1: Escrever os testes que falham**

Crie `src/mem.rs` só com o módulo de testes (o resto vem no Step 3). Os testes completos:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    const MI: usize = 1024 * 1024;
    const GI: u64 = 1024 * 1024 * 1024;

    #[test]
    fn parse_size_aceita_sufixos_sem_distinguir_caixa() {
        assert_eq!(parse_size("64M").unwrap(), 64 * MI);
        assert_eq!(parse_size("64m").unwrap(), 64 * MI);
        assert_eq!(parse_size("64MB").unwrap(), 64 * MI);
        assert_eq!(parse_size("64MiB").unwrap(), 64 * MI);
        assert_eq!(parse_size("1G").unwrap(), 1024 * MI);
        assert_eq!(parse_size("1 GiB").unwrap(), 1024 * MI);
        assert_eq!(parse_size("65536K").unwrap(), 64 * MI);
        assert_eq!(parse_size("67108864").unwrap(), 64 * MI);
        assert_eq!(parse_size(" 512M ").unwrap(), 512 * MI);
    }

    #[test]
    fn parse_size_recusa_o_que_nao_e_tamanho() {
        for ruim in ["", "abc", "-1", "1.5G", "G", "10X", "1 2", "0", "0M"] {
            assert!(parse_size(ruim).is_err(), "{ruim:?} deveria falhar");
        }
    }

    #[test]
    fn parse_size_recusa_overflow_e_minimo() {
        assert!(parse_size("999999999999999999999G").is_err());
        assert!(parse_size(&format!("{}G", usize::MAX)).is_err());
        let e = parse_size("16M").unwrap_err();
        assert!(e.contains("32 MiB"), "{e}");
        assert_eq!(parse_size("32M").unwrap(), MIN_LIMIT);
    }

    #[test]
    fn resolve_flag_vence_variavel_que_vence_padrao() {
        assert_eq!(resolve(Some(64 * MI), Some("1G")).unwrap(), 64 * MI);
        assert_eq!(resolve(None, Some("1G")).unwrap(), 1024 * MI);
        assert_eq!(resolve(None, None).unwrap(), DEFAULT_LIMIT);
        assert_eq!(resolve(None, Some("  ")).unwrap(), DEFAULT_LIMIT);
    }

    #[test]
    fn resolve_variavel_invalida_cita_o_nome() {
        let e = resolve(None, Some("muito")).unwrap_err();
        assert!(e.contains("MDV_MEMORY_BUFFER"), "{e}");
        assert!(resolve(Some(64 * MI), Some("muito")).is_ok(), "a flag vence; a variável nem é lida");
    }

    #[test]
    fn check_against_compara_com_o_limite() {
        assert!(check_against(10, 10).is_ok());
        assert_eq!(check_against(11, 10), Err(MemoryLimit { used: 11, limit: 10 }));
    }

    #[test]
    fn hard_cap_passa_do_limite_e_nao_da_overflow() {
        assert_eq!(hard_cap(512 * MI), 512 * MI + 256 * MI + 16 * MI);
        assert_eq!(hard_cap(usize::MAX), usize::MAX);
        assert!(hard_cap(MIN_LIMIT) > MIN_LIMIT);
    }

    #[test]
    fn fmt_size_usa_mib_ate_1_gib_e_gib_depois() {
        assert_eq!(fmt_size(512 * MI as u64), "512 MiB");
        assert_eq!(fmt_size(20 * GI), "20 GiB");
        assert_eq!(fmt_size(GI + GI / 2), "1,5 GiB");
        assert_eq!(fmt_size(100), "1 MiB");
        assert_eq!(fmt_size(u64::MAX).ends_with("GiB"), true);
    }

    #[test]
    fn flag_size_escolhe_g_quando_redondo() {
        assert_eq!(flag_size(2 * 1024 * MI), "2G");
        assert_eq!(flag_size(768 * MI), "768M");
    }

    #[test]
    fn describe_sugere_quatro_vezes_o_limite_dentro_da_memoria_livre() {
        let m = describe(512 * MI, Some(20 * GI));
        assert!(m.contains("limite de memória de 512 MiB"), "{m}");
        assert!(m.contains("20 GiB livres"), "{m}");
        assert!(m.contains("--memory-buffer 2G") && m.contains("MDV_MEMORY_BUFFER=2G"), "{m}");
    }

    #[test]
    fn describe_limita_a_sugestao_a_tres_quartos_da_memoria_livre() {
        let m = describe(512 * MI, Some(GI));
        assert!(m.contains("--memory-buffer 768M"), "{m}");
    }

    #[test]
    fn describe_sem_margem_nao_sugere_aumento() {
        let m = describe(512 * MI, Some(400 * MI as u64));
        assert!(m.contains("não há margem segura"), "{m}");
        assert!(!m.contains("--memory-buffer"), "{m}");
    }

    #[test]
    fn describe_sem_memoria_conhecida_usa_forma_generica() {
        let m = describe(512 * MI, None);
        assert!(m.contains("--memory-buffer <TAMANHO>"), "{m}");
        assert!(!m.contains("livres"), "{m}");
    }

    #[test]
    fn aviso_so_quando_o_limite_passa_da_memoria_livre() {
        assert!(warn_if_above(8 * 1024 * MI, Some(3 * GI)).unwrap().contains("8G"));
        assert!(warn_if_above(GI as usize, Some(3 * GI)).is_none());
        assert!(warn_if_above(8 * 1024 * MI, None).is_none());
    }

    #[test]
    fn meminfo_le_memavailable_em_kib() {
        let txt = "MemTotal:       23903888 kB\nMemFree:  1 kB\nMemAvailable:   21055460 kB\n";
        assert_eq!(parse_meminfo(txt), Some(21_055_460 * 1024));
        assert_eq!(parse_meminfo("MemTotal: 1 kB\n"), None);
        assert_eq!(parse_meminfo("MemAvailable: xyz kB\n"), None);
        assert_eq!(parse_meminfo(""), None);
    }

    #[test]
    fn contador_enxerga_um_vec_vivo() {
        let v = vec![1u8; 8 * MI];
        std::hint::black_box(&v);
        assert!(used() >= 8 * MI, "usado: {}", used());
    }

    #[test]
    fn check_input_recusa_arquivo_que_expandiria_demais() {
        assert!(check_input(1024).is_ok());
        assert!(check_input(u64::MAX).is_err());
        assert!(check_input((limit() as u64 / 4) + 1).is_err());
    }
}
```

Para o `fmt_size(u64::MAX)`, o clippy pode reclamar de `assert_eq!(..., true)`; use `assert!(fmt_size(u64::MAX).ends_with("GiB"))`.

- [ ] **Step 2: Rodar os testes e ver falhar**

Em `src/main.rs`, acrescente `mod mem;` (entre `mod image;` e `mod mermaid;`, mantendo a ordem alfabética existente).
Run: `cargo test --bin mdv mem:: 2>&1 | tail -15`
Expected: erro de compilação (`parse_size`, `MemoryLimit`, ... não definidos).

- [ ] **Step 3: Implementar `src/mem.rs`**

Coloque acima do módulo `tests`:

```rust
//! Limite de memória do processo: um alocador que conta os bytes vivos e pontos de checagem
//! que interrompem o trabalho antes de a máquina ficar sem memória.

use std::alloc::{GlobalAlloc, Layout, System};
use std::fmt;
use std::ptr::null_mut;
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};
use std::sync::OnceLock;

const MIB: usize = 1024 * 1024;
const GIB: usize = 1024 * MIB;

/// Limite padrão de memória do processo.
pub const DEFAULT_LIMIT: usize = 512 * MIB;
/// Menor limite aceito: abaixo disso o `syntect` sozinho (~50 MiB) já estouraria.
pub const MIN_LIMIT: usize = 32 * MIB;

/// Bytes vivos no processo (alocados e ainda não liberados).
static USED: AtomicUsize = AtomicUsize::new(0);
static LIMIT: AtomicUsize = AtomicUsize::new(DEFAULT_LIMIT);
static HARD_CAP: AtomicUsize = AtomicUsize::new(hard_cap(DEFAULT_LIMIT));
/// Memória disponível do computador, lida uma vez em `init`.
static AVAILABLE: OnceLock<Option<u64>> = OnceLock::new();

/// Teto duro do alocador: acima disso a alocação é recusada e o Rust aborta. Só é alcançado por
/// um ponto de código sem checagem que aloque de uma vez; no caminho normal `check` para antes.
pub const fn hard_cap(limit: usize) -> usize {
    limit.saturating_add(limit / 2).saturating_add(16 * MIB)
}

/// Alocador que conta os bytes vivos e recusa alocações acima de `HARD_CAP`.
pub struct CountingAlloc;

/// Soma `n` bytes ao contador; `false` (sem alterar nada) se passaria do teto duro.
fn reserve(n: usize) -> bool {
    let before = USED.fetch_add(n, Relaxed);
    if before.saturating_add(n) > HARD_CAP.load(Relaxed) {
        USED.fetch_sub(n, Relaxed);
        return false;
    }
    true
}

// SAFETY: cada método repassa o mesmo contrato ao `System`; o contador só soma e subtrai o
// tamanho que o chamador já informou em `Layout`.
unsafe impl GlobalAlloc for CountingAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if !reserve(layout.size()) {
            return null_mut();
        }
        let p = unsafe { System.alloc(layout) };
        if p.is_null() {
            USED.fetch_sub(layout.size(), Relaxed);
        }
        p
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        if !reserve(layout.size()) {
            return null_mut();
        }
        let p = unsafe { System.alloc_zeroed(layout) };
        if p.is_null() {
            USED.fetch_sub(layout.size(), Relaxed);
        }
        p
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) };
        USED.fetch_sub(layout.size(), Relaxed);
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let old = layout.size();
        if new_size > old && !reserve(new_size - old) {
            return null_mut();
        }
        let p = unsafe { System.realloc(ptr, layout, new_size) };
        if p.is_null() {
            if new_size > old {
                USED.fetch_sub(new_size - old, Relaxed);
            }
        } else if new_size < old {
            USED.fetch_sub(old - new_size, Relaxed);
        }
        p
    }
}

pub fn used() -> usize {
    USED.load(Relaxed)
}

pub fn limit() -> usize {
    LIMIT.load(Relaxed)
}

/// Define o limite (e o teto duro correspondente). Chamado uma vez no início do `main`.
pub fn set_limit(bytes: usize) {
    LIMIT.store(bytes, Relaxed);
    HARD_CAP.store(hard_cap(bytes), Relaxed);
}

/// O processo passou do limite de memória.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryLimit {
    pub used: usize,
    pub limit: usize,
}

impl fmt::Display for MemoryLimit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&describe(self.limit, available()))
    }
}

pub fn check_against(used: usize, limit: usize) -> Result<(), MemoryLimit> {
    if used > limit {
        Err(MemoryLimit { used, limit })
    } else {
        Ok(())
    }
}

/// Ponto de checagem dos laços pesados: um teste do contador, sem I/O.
pub fn check() -> Result<(), MemoryLimit> {
    check_against(used(), limit())
}

pub fn exceeded() -> bool {
    used() > limit()
}

/// Recusa, antes de ler, um arquivo que expandiria além do limite (a renderização costuma
/// multiplicar o tamanho por 10 a 50; aqui basta ×4 para barrar os casos óbvios).
pub fn check_input(len: u64) -> Result<(), MemoryLimit> {
    let need = usize::try_from(len).unwrap_or(usize::MAX).saturating_mul(4);
    check_against(need, limit())
}

/// Interpreta `256M`, `1G`, `512MiB`, `65536K` ou um número de bytes (múltiplos de 1024).
pub fn parse_size(s: &str) -> Result<usize, String> {
    let bad = || format!("tamanho inválido: {s} (exemplos: 256M, 1G)");
    let t = s.trim();
    let split = t.find(|c: char| !c.is_ascii_digit()).unwrap_or(t.len());
    let (digits, suffix) = t.split_at(split);
    let n: usize = digits.parse().map_err(|_| bad())?;
    let mult = match suffix.trim().to_ascii_lowercase().as_str() {
        "" | "b" => 1,
        "k" | "kb" | "kib" => 1024,
        "m" | "mb" | "mib" => MIB,
        "g" | "gb" | "gib" => GIB,
        _ => return Err(bad()),
    };
    let bytes = n.checked_mul(mult).ok_or_else(|| format!("tamanho grande demais: {s}"))?;
    if bytes < MIN_LIMIT {
        return Err(format!("tamanho pequeno demais: {s} (mínimo {})", fmt_size(MIN_LIMIT as u64)));
    }
    Ok(bytes)
}

/// Flag vence variável de ambiente, que vence o padrão.
pub fn resolve(flag: Option<usize>, env: Option<&str>) -> Result<usize, String> {
    match (flag, env) {
        (Some(f), _) => Ok(f),
        (None, Some(e)) if !e.trim().is_empty() => {
            parse_size(e).map_err(|m| format!("MDV_MEMORY_BUFFER: {m}"))
        }
        _ => Ok(DEFAULT_LIMIT),
    }
}

/// `512 MiB`, `20 GiB`, `1,5 GiB`.
pub fn fmt_size(bytes: u64) -> String {
    const MI: u64 = 1 << 20;
    const GI: u64 = 1 << 30;
    if bytes >= GI {
        let tenths = (bytes as f64 / GI as f64 * 10.0).round() as u64;
        if tenths % 10 == 0 {
            format!("{} GiB", tenths / 10)
        } else {
            format!("{},{} GiB", tenths / 10, tenths % 10)
        }
    } else {
        format!("{} MiB", ((bytes + MI / 2) / MI).max(1))
    }
}

/// Forma aceita por `--memory-buffer`: `2G` quando redondo, senão `768M`.
pub fn flag_size(bytes: usize) -> String {
    if bytes % GIB == 0 {
        format!("{}G", bytes / GIB)
    } else {
        format!("{}M", bytes.div_ceil(MIB))
    }
}

/// Novo limite sugerido: 4× o atual, no máximo 3/4 da memória livre; `None` se não passa do atual.
fn suggest_limit(limit: usize, free: u64) -> Option<usize> {
    let cap = usize::try_from(free / 4 * 3).unwrap_or(usize::MAX);
    let s = limit.saturating_mul(4).min(cap) / MIB * MIB;
    (s > limit).then_some(s)
}

/// Mensagem mostrada no lugar do documento quando o limite é atingido.
pub fn describe(limit: usize, available: Option<u64>) -> String {
    let head = format!("limite de memória de {} atingido ao processar este arquivo.", fmt_size(limit as u64));
    match available {
        None => format!("{head} Para ler mais, use --memory-buffer <TAMANHO> (por exemplo 2G)."),
        Some(free) => match suggest_limit(limit, free) {
            Some(s) => format!(
                "{head} O computador tem cerca de {} livres; para ler mais, use --memory-buffer {} (ou MDV_MEMORY_BUFFER={}).",
                fmt_size(free),
                flag_size(s),
                flag_size(s)
            ),
            None => format!(
                "{head} O computador tem só cerca de {} livres; não há margem segura para aumentar o limite.",
                fmt_size(free)
            ),
        },
    }
}

/// Aviso (não bloqueia) quando o limite pedido passa da memória livre.
pub fn warn_if_above(limit: usize, available: Option<u64>) -> Option<String> {
    let free = available?;
    (limit as u64 > free).then(|| {
        format!(
            "aviso: o limite de memória de {} é maior que a memória livre do computador ({}); o sistema pode ficar lento ou encerrar o programa.",
            flag_size(limit),
            fmt_size(free)
        )
    })
}

/// `MemAvailable` de `/proc/meminfo`, em bytes.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub fn parse_meminfo(text: &str) -> Option<u64> {
    let line = text.lines().find(|l| l.starts_with("MemAvailable:"))?;
    let kib: u64 = line.split_whitespace().nth(1)?.parse().ok()?;
    kib.checked_mul(1024)
}

/// Memória disponível do computador, se o sistema souber dizer.
#[cfg(target_os = "linux")]
pub fn available_bytes() -> Option<u64> {
    parse_meminfo(&std::fs::read_to_string("/proc/meminfo").ok()?)
}

#[cfg(not(target_os = "linux"))]
pub fn available_bytes() -> Option<u64> {
    None
}

/// Lê a memória disponível uma única vez (a mensagem de erro não faz I/O).
pub fn init() {
    let _ = AVAILABLE.set(available_bytes());
}

pub fn available() -> Option<u64> {
    AVAILABLE.get().copied().flatten()
}
```

Em `src/main.rs`, logo depois dos `mod` e `use`:

```rust
#[global_allocator]
static ALLOC: mem::CountingAlloc = mem::CountingAlloc;
```

- [ ] **Step 4: Rodar os testes e ver passar**

Run: `(ulimit -v 6000000; cargo test 2>&1 | grep -E "^test result|FAILED|panicked|error")`
Expected: tudo verde, inclusive os testes antigos (agora com o alocador contador e teto duro de 784 MiB). Se `contador_enxerga_um_vec_vivo` falhar com `usado` enorme, há underflow no contador: revise os `fetch_sub`.

- [ ] **Step 5: Clippy e commit**

Run: `cargo clippy --all-targets 2>&1 | tail -15`
Se o clippy reclamar de `as` (casts) em `fmt_size`, acrescente `#[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation, clippy::cast_sign_loss)]` na função (só se aparecer o aviso).

```bash
git add src/mem.rs src/main.rs
git commit -m "feat(mem): alocador contador, limite de memória e mensagens (F7.9)"
```

---

### Task 2: Flag `--memory-buffer`, variável de ambiente, aviso e ajuda

**Files:**
- Modify: `src/cli.rs`, `src/main.rs`
- Test: `src/cli.rs`, `tests/cli.rs`

**Interfaces:**
- Consumes: `mem::parse_size`, `mem::resolve`, `mem::set_limit`, `mem::init`, `mem::available`, `mem::warn_if_above` (Task 1).
- Produces: `Cli.memory_buffer: Option<usize>` (bytes, já validado ≥ 32 MiB); `App.notice` recebe o aviso na TUI.

- [ ] **Step 1: Escrever os testes que falham**

Em `src/cli.rs` (módulo `tests`):

```rust
    #[test]
    fn memory_buffer_aceita_tamanhos() {
        assert_eq!(run(&["--memory-buffer", "256M", "a.md"]).unwrap().memory_buffer, Some(256 * 1024 * 1024));
        assert_eq!(run(&["--memory-buffer", "1g", "a.md"]).unwrap().memory_buffer, Some(1024 * 1024 * 1024));
        assert_eq!(run(&["a.md"]).unwrap().memory_buffer, None);
    }

    #[test]
    fn memory_buffer_invalido_e_erro_de_uso() {
        for v in ["0", "-1", "1.5G", "abc", "", "16M", "999999999999999999999G"] {
            let e = run(&["--memory-buffer", v, "a.md"]).unwrap_err();
            assert!(e.contains("--memory-buffer"), "{v:?}: {e}");
        }
        assert!(run(&["--memory-buffer"]).is_err());
    }
```

Em `tests/cli.rs`:

```rust
#[test]
fn memory_buffer_invalido_e_erro_de_uso() {
    let out = mdv(&["--memory-buffer", "abc", "-"], "# x\n");
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("--memory-buffer"));
}

fn mdv_env(args: &[&str], stdin: &str, env: Option<&str>) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_mdv"));
    cmd.args(args).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    if let Some(v) = env {
        cmd.env("MDV_MEMORY_BUFFER", v);
    } else {
        cmd.env_remove("MDV_MEMORY_BUFFER");
    }
    let mut child = cmd.spawn().expect("mdv deve iniciar");
    let _ = child.stdin.take().expect("stdin").write_all(stdin.as_bytes());
    child.wait_with_output().expect("mdv deve terminar")
}

#[test]
fn variavel_de_ambiente_invalida_e_erro_de_uso_que_cita_a_variavel() {
    let out = mdv_env(&["-p", "-"], "# x\n", Some("muito"));
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("MDV_MEMORY_BUFFER"));
}

#[test]
fn flag_valida_vence_variavel_invalida() {
    let out = mdv_env(&["-p", "--memory-buffer", "256M", "-"], "# x\n", Some("muito"));
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
}

#[test]
fn limite_maior_que_a_memoria_livre_avisa_no_stderr_e_segue() {
    // Só faz sentido onde o mdv sabe a memória livre (Linux).
    if !std::path::Path::new("/proc/meminfo").exists() {
        return;
    }
    let out = mdv(&["-p", "--memory-buffer", "999999G", "-"], "# x\n");
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("aviso: o limite de memória"));
    assert!(String::from_utf8_lossy(&out.stdout).contains('x'));
}
```

(`Command` já está importado em `tests/cli.rs`: `use std::process::{Command, Output, Stdio};`.)

- [ ] **Step 2: Rodar os testes e ver falhar**

Run: `cargo test 2>&1 | tail -15`
Expected: erro de compilação (`memory_buffer` não existe em `Cli`).

- [ ] **Step 3: Implementar**

`src/cli.rs`: `use crate::{mem, render};` (hoje só importa `render`); campo `pub memory_buffer: Option<usize>,` em `Cli`; `memory_buffer: None,` no literal de `parse`; e o braço:

```rust
            "--memory-buffer" => {
                let v = value(&mut args, &a)?;
                cli.memory_buffer =
                    Some(mem::parse_size(&v).map_err(|e| format!("--memory-buffer: {e}"))?);
            }
```

`src/main.rs`, em `run_cli`, logo depois do `match cli::parse(...)` que produz `cli` e antes de `latest_dir`:

```rust
    let limit = mem::resolve(cli.memory_buffer, std::env::var("MDV_MEMORY_BUFFER").ok().as_deref())
        .map_err(Failure::Usage)?;
    mem::set_limit(limit);
    mem::init();
    let mem_warning = mem::warn_if_above(limit, mem::available());
```

Depois que `interactive` for calculado:

```rust
    if !interactive {
        if let Some(w) = &mem_warning {
            eprintln!("mdv: {w}");
        }
    }
```

Nos dois lugares que criam `App` (o do `--follow` e o do fim de `run_cli`), depois de `App::new(...)`, atribua o aviso: troque `run_tui(App::new(Source::Stdin, String::new(), width, &cli).with_stream(rx))` por

```rust
        let mut app = App::new(Source::Stdin, String::new(), width, &cli).with_stream(rx);
        app.notice = mem_warning;
        return run_tui(app);
```

e, no outro, `let mut app = App::new(source, md, width, &cli); app.notice = mem_warning;` (o `mem_warning` é movido só no ramo interativo; no ramo `--plain` ele já foi impresso acima. Se o compilador reclamar de uso depois do move, use `.clone()` no primeiro).

`HELP`: nas OPÇÕES, depois de `--table <N>`:

```
        --memory-buffer <T>  limite de memória (padrão: 512M; ex.: 256M, 2G; ou $MDV_MEMORY_BUFFER)
```

- [ ] **Step 4: Rodar os testes e ver passar**

Run: `(ulimit -v 6000000; cargo test 2>&1 | grep -E "^test result|FAILED|panicked|error")`
Expected: tudo verde. O teste de aviso no stderr só roda em Linux.

- [ ] **Step 5: Clippy e commit**

Run: `cargo clippy --all-targets 2>&1 | tail -15`

```bash
git add src/cli.rs src/main.rs tests/cli.rs
git commit -m "feat(cli): --memory-buffer e MDV_MEMORY_BUFFER com aviso de memória livre (F7.9)"
```

---

### Task 3: Pontos de checagem e testes de estouro real

**Files:**
- Modify: `src/render.rs`, `src/data.rs`, `src/main.rs`
- Test: `src/main.rs`, `tests/cli.rs`

**Interfaces:**
- Consumes: `mem::check`, `mem::exceeded`, `mem::check_input`, `mem::MemoryLimit` (Task 1); `--memory-buffer` (Task 2).
- Produces: `App::stop_stream_for_memory(&mut self, over: Option<mem::MemoryLimit>)`.

- [ ] **Step 1: Escrever os testes que falham**

Em `tests/cli.rs`:

```rust
#[test]
fn markdown_que_expande_alem_do_limite_para_com_mensagem() {
    // 4 MiB de parágrafos de uma letra: ~1,4 milhão de parágrafos, centenas de MiB renderizados.
    let md = "a\n\n".repeat(4 * 1024 * 1024 / 3);
    let out = mdv(&["-p", "--memory-buffer", "32M", "-"], &md);
    assert!(out.status.success(), "não pode abortar: {}", String::from_utf8_lossy(&out.stderr));
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("limite de memória de 32 MiB atingido"), "{}", &text[..text.len().min(300)]);
    assert!(text.contains("--memory-buffer"), "{}", &text[..text.len().min(300)]);
    assert!(text.lines().count() < 5, "o documento parcial é descartado");
}

#[test]
fn arquivo_grande_demais_para_o_limite_e_recusado_antes_de_ler() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("grande.md");
    std::fs::write(&p, "x".repeat(9 * 1024 * 1024)).unwrap();
    let out = mdv(&["-p", "--memory-buffer", "32M", p.to_str().unwrap()], "");
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("limite de memória de 32 MiB"));
}

#[test]
fn json_com_chave_gigante_respeita_o_limite_padrao() {
    // ~3 MiB de chave com 100 mil filhos: o flatten antigo copiava a chave para cada filho.
    let filhos: String = (0..100_000).map(|i| format!("\"k{i}\":0")).collect::<Vec<_>>().join(",");
    let json = format!("[{{\"{}\":{{{filhos}}}}}]", "A".repeat(1_000_000));
    let out = mdv(&["json", "-p", "-"], &json);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
}
```

Em `src/main.rs` (módulo `tests`):

```rust
    #[test]
    fn stdin_para_de_acumular_quando_a_memoria_acaba() {
        let (_tx, rx) = std::sync::mpsc::channel::<String>();
        let mut app = test_app(Source::Stdin, "").with_stream(rx);
        app.stop_stream_for_memory(None);
        assert!(app.stream.is_some(), "sem estouro, segue lendo");
        app.stop_stream_for_memory(Some(mem::MemoryLimit { used: 2, limit: 1 }));
        assert!(app.stream.is_none());
        assert!(app.notice.as_deref().is_some_and(|n| n.contains("leitura interrompida")));
        assert!(!app.images.mermaid_held(), "libera os diagramas");
    }
```

- [ ] **Step 2: Rodar os testes e ver falhar**

Run: `(ulimit -v 6000000; cargo test --test cli markdown_que_expande 2>&1 | tail -12)`
Expected: FAIL por abort (`memory allocation of ... bytes failed`) ou por falta da mensagem — o `render_with` ainda não checa. O teto duro do alocador (32 MiB × 1,5 + 16 = 64 MiB) impede que a máquina sofra; é exatamente o papel dele.

- [ ] **Step 3: Implementar as checagens**

`src/render.rs` — no topo, junto dos outros `use crate::...`: `use crate::mem;`. Helper (perto de `error_rendered`):

```rust
/// Erro de memória como documento, se o limite foi passado.
pub fn memory_error() -> Option<Rendered> {
    mem::check().err().map(|e| error_rendered(&e.to_string()))
}
```

`render_with`:

```rust
    let mut r = Renderer::new(opts.width, opts.images);
    for ev in Parser::new_ext(md, parse_opts) {
        r.handle(sanitize_event(ev));
        if let Some(err) = memory_error() {
            return err;
        }
    }
    r.finish()
```

`render_code_only` e `render_table_only`: troque `r.out` final por `memory_error().unwrap_or(r.out)`.

Laço de `layout_table`: primeira linha do corpo do `for (ri, row) in t.rows.iter().enumerate()`:

```rust
        if mem::exceeded() {
            break;
        }
```

Laço de `render_code` (`for line in LinesWithEndings::from(&block.buf)`): primeira linha:

```rust
            if mem::exceeded() {
                break;
            }
```

(Em ambos o laço só para de crescer; quem devolve o erro é a checagem de evento seguinte em `render_with`, ou o `memory_error()` de `render_code_only`/`render_table_only`. No zoom (`TableData::draw`) uma tabela interrompida fica truncada em silêncio: o zoom só redesenha dados que já couberam na memória.)

`src/data.rs` — `use crate::mem;` e, perto de `clip_cell`:

```rust
use crate::render::memory_error;
```

(se `error_rendered` já vem de `crate::render`, junte no mesmo `use`). Em `render_json`:

```rust
    let root = match parse_json(text.trim_start_matches('\u{feff}')) {
        Ok(v) => v,
        Err(e) => return error_rendered(&e),
    };
    if let Some(err) = memory_error() {
        return err;
    }
```

e troque o `match view { ... }` final por `let out = match view { ... }; memory_error().unwrap_or(out)`. Em `records_to_rows`, o iterador dos itens passa a ser `items.iter().take(head).take_while(|_| !mem::exceeded())`. Em `render_table_data`, no começo do corpo do `for rec in rdr.records()`:

```rust
        if let Some(err) = memory_error() {
            return err;
        }
```

e o `r` final: `memory_error().unwrap_or(r)` antes de devolver.

`src/main.rs`:

```rust
fn read_markdown_file(path: &Path) -> io::Result<String> {
    let meta = fs::metadata(path)?;
    if !meta.is_file() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "não é um arquivo regular"));
    }
    mem::check_input(meta.len())
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
    read_limited(fs::File::open(path)?, MAX_INPUT_BYTES)
}
```

Método novo de `App` (junto de `drain_stream`) e chamada no fim de `drain_stream`, depois do laço `loop { rx.try_recv() ... }` e antes do `if finished`:

```rust
    /// Para de acumular o stdin quando a memória passou do limite (`--follow`).
    fn stop_stream_for_memory(&mut self, over: Option<mem::MemoryLimit>) {
        if let Some(e) = over {
            if self.stream.take().is_some() {
                self.images.set_mermaid_hold(false);
                self.notice = Some(format!("leitura interrompida: {e}"));
                self.dirty = true;
            }
        }
    }
```

```rust
        self.stop_stream_for_memory(mem::check().err());
```

Se o compilador reclamar de empréstimo de `rx` (a variável `rx` empresta `self.stream`), mova a chamada para depois do último uso de `rx` ou reestruture com `let rx = self.stream.as_ref()...` terminando o laço antes.

- [ ] **Step 4: Rodar os testes e ver passar**

Run: `(ulimit -v 6000000; cargo test 2>&1 | grep -E "^test result|FAILED|panicked|error")`
Expected: tudo verde. Os testes de estouro rodam o binário com limite de 32 MiB: o pico fica perto de 32–40 MiB. Se o teste do Markdown ainda abortar, o teto duro está pegando antes da checagem: aumente a frequência (confirme que a checagem é por evento, não por bloco).

Validação manual (cuidado de memória: o limite é pequeno de propósito):

```bash
(ulimit -v 2000000; yes 'a' | head -c 4000000 | sed 's/$/\n/' | cargo run --release -- -p --memory-buffer 32M - | head -3)
(ulimit -v 2000000; cargo run --release -- -p --memory-buffer 8M README.md)   # erro de uso: mínimo 32 MiB
```

- [ ] **Step 5: Clippy e commit**

Run: `cargo clippy --all-targets 2>&1 | tail -15`

```bash
git add src/render.rs src/data.rs src/main.rs tests/cli.rs
git commit -m "feat(mem): checagens de limite no renderizador, nos dados e na leitura (F7.9)"
```

---

### Task 4: Documentação

**Files:**
- Modify: `README.md`, `CLAUDE.md`, `PLANO.md`, `docs/superpowers/specs/2026-10-15-fase-7-limite-de-memoria-design.md`

- [ ] **Step 1: README**

- Bloco de ajuda (`<!-- mdv:help:start -->`): copie as mesmas linhas do `HELP` (a nova opção `--memory-buffer <T>`).
- Seção nova "Limite de memória", depois de "JSON, CSV e TSV":

```markdown
## Limite de memória

Por segurança o `mdv` usa no máximo **512 MiB** de memória. Se um arquivo precisar de mais (um Markdown muito grande ou um JSON hostil), ele para e mostra no lugar do documento uma mensagem com o limite, a memória livre do computador e o valor sugerido. Para ler arquivos maiores:

```bash
mdv --memory-buffer 2G grande.md
MDV_MEMORY_BUFFER=2G mdv grande.md      # a flag vence a variável
```

Aceita `256M`, `1G`, `512MiB`, `65536K` (múltiplos de 1024); o mínimo é 32 MiB. Se o valor passar da memória livre do computador, o `mdv` avisa e segue. A memória livre é detectada no Linux; no macOS e no Windows a mensagem só não traz o número.
```

- Em "Limitações conhecidas", acrescente: "- A memória livre do computador só é informada no Linux."

- [ ] **Step 2: CLAUDE.md**

Em "Arquitetura", depois do item de `src/stream.rs`, acrescente:

```markdown
- `src/mem.rs`: limite de memória do processo (`--memory-buffer`, `MDV_MEMORY_BUFFER`, padrão 512 MiB). `CountingAlloc` é o `#[global_allocator]` (conta bytes vivos, teto duro `limite×1,5+16 MiB`); `check()`/`exceeded()` são os pontos de checagem usados por `render_with`, `layout_table`, `render_code`, `data.rs` e `main.rs`; ao estourar devolvem `error_rendered(MemoryLimit)`. Testes unitários **não** alteram o limite global (rodam em paralelo); o estouro real é testado pelo binário em `tests/cli.rs`.
```

Na seção "Memória: limitar a aplicação...", acrescente ao fim: "O `mdv` já tem esse fail-safe (`src/mem.rs`): novo laço que cresça com a entrada deve chamar `mem::check()`/`mem::exceeded()`."

- [ ] **Step 3: PLANO.md e spec**

- Em `PLANO.md`, depois de F7.8, acrescente: `- [x] **F7.9 Limite de memória** — ... Plano detalhado: [...]` com o resumo da spec (alocador contador, `--memory-buffer`/`MDV_MEMORY_BUFFER`, padrão 512 MiB, mínimo 32 MiB, erro com dica ao estourar, aviso de memória livre; só Linux informa a memória livre). Na linha "Ordem sugerida" da Fase 7 acrescente `F7.9 (independente; protege todos os itens seguintes)` logo depois de `F7.8`.
- Na spec, troque o aviso de valor alto pela forma final: `aviso: o limite de memória de 8G é maior que a memória livre do computador (3,1 GiB); o sistema pode ficar lento ou encerrar o programa.`

- [ ] **Step 4: Verificação final e commit**

Run: `(ulimit -v 6000000; cargo test 2>&1 | grep -E "^test result|FAILED|panicked") && cargo clippy --all-targets 2>&1 | tail -5`
Expected: verde, sem avisos.

```bash
git add README.md CLAUDE.md PLANO.md docs/superpowers
git commit -m "docs: limite de memória (--memory-buffer) e F7.9"
```

---

## Self-Review

**Cobertura da spec:**
- Contagem por alocador, teto duro, `realloc` no lugar, `unsafe` restrito → Task 1.
- `parse_size`, `resolve` (flag > env > padrão), mínimo de 32 MiB, erros de uso → Tasks 1 e 2.
- Mensagens (com margem, sem margem, desconhecida) e aviso de valor alto → Task 1 (funções puras) e Task 2 (entrega: stderr no `--plain`, barra na TUI).
- Tabela de pontos de checagem: `render_with`, `layout_table`, `render_code`, `render_json`, `records_to_rows`, `render_table_data`, `read_markdown_file`, `drain_stream` → Task 3. `render_code_only` e `render_table_only` também.
- Documentação (`--help`, README, CLAUDE.md, PLANO.md) → Tasks 2 e 4.
- Testes: unitários puros (Task 1), precedência e erro de uso (Tasks 1–2), estouro real, arquivo grande, JSON hostil (Task 3).

**Placeholders:** nenhum "TBD". Dois pontos dependem do compilador e trazem a saída: o possível `.clone()` de `mem_warning` (Task 2) e o empréstimo de `rx` em `drain_stream` (Task 3).

**Consistência de tipos:** `limit`/`used`/`bytes` são `usize` em todo o módulo, e `available`/`fmt_size` usam `u64` (memória do sistema), com `as u64` nas fronteiras. `MemoryLimit { used, limit }` é igual em `check_against`, `check_input` (onde `used` guarda o tamanho estimado) e em `stop_stream_for_memory`. `resolve` recebe `Option<usize>` (já validado pelo `cli`) e `Option<&str>`.

**Review Focus → testes:** 4 MiB que expande (`markdown_que_expande_alem_do_limite_para_com_mensagem`); arquivo × 4 > limite (`arquivo_grande_demais_para_o_limite_e_recusado_antes_de_ler`, `check_input_recusa_arquivo_que_expandiria_demais`); valores inválidos (`parse_size_recusa_*`, `memory_buffer_invalido_e_erro_de_uso`); ambiente (`variavel_de_ambiente_invalida_*`, `flag_valida_vence_variavel_invalida`); memória desconhecida (`describe_sem_memoria_conhecida_usa_forma_generica`, `meminfo_le_memavailable_em_kib`); aviso (`aviso_so_quando_o_limite_passa_da_memoria_livre`, `limite_maior_que_a_memoria_livre_avisa_no_stderr_e_segue`); `--follow` (`stdin_para_de_acumular_quando_a_memoria_acaba`); contador (`contador_enxerga_um_vec_vivo`).
