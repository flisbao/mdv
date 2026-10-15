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

/// Estimativa do pico do `pulldown-cmark`, que monta a árvore do documento inteiro antes de
/// emitir o primeiro evento (medido: ~24× o texto no pior caso, parágrafos de uma letra).
/// Barra, antes de analisar, o texto cuja árvore passaria do limite.
pub fn check_parse(len: usize) -> Result<(), MemoryLimit> {
    check_against(used().saturating_add(len.saturating_mul(24)), limit())
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
        if tenths.is_multiple_of(10) {
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
    if bytes.is_multiple_of(GIB) {
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
        assert!(fmt_size(u64::MAX).ends_with("GiB"));
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
    fn check_parse_conta_a_arvore_do_parser() {
        assert!(check_parse(1024).is_ok());
        assert!(check_parse(usize::MAX).is_err());
        assert!(check_parse(limit() / 24 + 1).is_err());
    }

    #[test]
    fn check_input_recusa_arquivo_que_expandiria_demais() {
        assert!(check_input(1024).is_ok());
        assert!(check_input(u64::MAX).is_err());
        assert!(check_input((limit() as u64 / 4) + 1).is_err());
    }
}
