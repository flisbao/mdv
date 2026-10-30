//! Seletor de arquivos (`mdv <pasta>`): lista os `.md`, `.msg`, `.yaml`/`.yml` e `.html`/`.htm` da pasta por mtime, com filtro fuzzy.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Teto de arquivos listados (protege a memória em pastas enormes).
const MAX_ENTRIES: usize = 5000;

#[derive(Clone)]
pub struct Entry {
    pub name: String,
    pub path: PathBuf,
    pub mtime: Option<SystemTime>,
}

/// Estado do seletor: lista completa, consulta digitada, índices filtrados e seleção.
#[derive(Clone)]
pub struct Picker {
    pub dir: PathBuf,
    pub entries: Vec<Entry>,
    pub query: String,
    pub filtered: Vec<usize>,
    pub selected: usize,
    /// Verdadeiro se a lista parou no teto de `MAX_ENTRIES`.
    pub truncated: bool,
}

/// Arquivos listados no seletor: Markdown, e-mails `.msg`, YAML e HTML (JSON e CSV/TSV ficam de fora).
fn is_listed(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    n.ends_with(".md")
        || n.ends_with(".markdown")
        || n.ends_with(".msg")
        || n.ends_with(".yaml")
        || n.ends_with(".yml")
        || n.ends_with(".html")
        || n.ends_with(".htm")
}

impl Picker {
    /// Lê `dir` (sem descer em subpastas), do mais novo ao mais antigo.
    pub fn scan(dir: &Path) -> io::Result<Picker> {
        let mut entries = Vec::new();
        let mut truncated = false;
        for e in fs::read_dir(dir)? {
            let Ok(e) = e else { continue };
            let name = crate::render::sanitize(&e.file_name().to_string_lossy()).into_owned();
            if !is_listed(&name) {
                continue;
            }
            // `metadata` segue links simbólicos: só entra o que é arquivo regular.
            let Ok(meta) = fs::metadata(e.path()) else { continue };
            if !meta.is_file() {
                continue;
            }
            if entries.len() >= MAX_ENTRIES {
                truncated = true;
                break;
            }
            entries.push(Entry { name, path: e.path(), mtime: meta.modified().ok() });
        }
        entries.sort_by(|a, b| b.mtime.cmp(&a.mtime).then_with(|| a.name.cmp(&b.name)));
        let mut p = Picker {
            dir: dir.to_path_buf(),
            entries,
            query: String::new(),
            filtered: Vec::new(),
            selected: 0,
            truncated,
        };
        p.refilter();
        Ok(p)
    }

    /// Recalcula `filtered` a partir de `query`; sem consulta mantém a ordem por mtime.
    pub fn refilter(&mut self) {
        if self.query.is_empty() {
            self.filtered = (0..self.entries.len()).collect();
        } else {
            let mut scored: Vec<(i32, usize)> = self
                .entries
                .iter()
                .enumerate()
                .filter_map(|(i, e)| fuzzy_score(&self.query, &e.name).map(|s| (s, i)))
                .collect();
            scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
            self.filtered = scored.into_iter().map(|(_, i)| i).collect();
        }
        self.selected = 0;
    }

    pub fn push(&mut self, c: char) {
        if self.query.chars().count() < 200 && !c.is_control() {
            self.query.push(c);
            self.refilter();
        }
    }

    pub fn pop(&mut self) {
        self.query.pop();
        self.refilter();
    }

    pub fn step(&mut self, delta: isize) {
        let n = self.filtered.len();
        if n == 0 {
            return;
        }
        self.selected = self.selected.saturating_add_signed(delta).min(n - 1);
    }

    pub fn current(&self) -> Option<&Entry> {
        self.filtered.get(self.selected).map(|&i| &self.entries[i])
    }
}

/// Casa `query` como subsequência (sem diferenciar caixa); maior é melhor. Letras seguidas e
/// no começo de uma palavra pontuam mais, e nomes curtos ganham de longos.
pub fn fuzzy_score(query: &str, name: &str) -> Option<i32> {
    let hay: Vec<char> = name.chars().flat_map(char::to_lowercase).collect();
    let mut pos = 0;
    let mut score = 0i32;
    let mut prev: Option<usize> = None;
    for qc in query.chars().flat_map(char::to_lowercase) {
        let found = hay[pos..].iter().position(|&c| c == qc)? + pos;
        score += 10;
        if prev.is_some_and(|p| p + 1 == found) {
            score += 15;
        }
        if found == 0 || !hay[found - 1].is_alphanumeric() {
            score += 8;
        }
        prev = Some(found);
        pos = found + 1;
    }
    let len = i32::try_from(hay.len()).unwrap_or(i32::MAX);
    Some(score - len.min(100) / 4)
}

/// Idade legível: "agora", "5 min", "3 h", "2 d".
pub fn age(mtime: Option<SystemTime>, now: SystemTime) -> String {
    let Some(secs) = mtime.and_then(|t| now.duration_since(t).ok()).map(|d| d.as_secs()) else {
        return "—".to_string();
    };
    match secs {
        0..=59 => "agora".to_string(),
        60..=3599 => format!("{} min", secs / 60),
        3600..=86_399 => format!("{} h", secs / 3600),
        _ => format!("{} d", secs / 86_400),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn fuzzy_exige_subsequencia_e_prefere_consecutivas() {
        assert!(fuzzy_score("xyz", "plano.md").is_none());
        let a = fuzzy_score("pl", "plano.md").unwrap();
        let b = fuzzy_score("pl", "p-x-l.md").unwrap();
        assert!(a > b);
        assert!(fuzzy_score("PLANO", "plano.md").is_some(), "sem diferenciar caixa");
    }

    #[test]
    fn scan_lista_so_md_regular_por_mtime() {
        let d = tempfile::tempdir().unwrap();
        fs::write(d.path().join("velho.md"), "a").unwrap();
        fs::write(d.path().join("nota.txt"), "a").unwrap();
        fs::create_dir(d.path().join("sub.md")).unwrap();
        let c = fs::File::create(d.path().join("c.msg")).unwrap();
        c.set_modified(SystemTime::now() + Duration::from_secs(50)).unwrap();
        let f = fs::File::create(d.path().join("novo.md")).unwrap();
        f.set_modified(SystemTime::now() + Duration::from_secs(100)).unwrap();
        let p = Picker::scan(d.path()).unwrap();
        let names: Vec<_> = p.entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["novo.md", "c.msg", "velho.md"]);
    }

    #[test]
    fn lista_yaml_e_yml_mas_nao_json_nem_csv() {
        let d = tempfile::tempdir().unwrap();
        for n in ["a.md", "b.yaml", "c.YML", "d.json", "e.csv", "f.txt", "g.tsv"] {
            fs::write(d.path().join(n), "a").unwrap();
        }
        let p = Picker::scan(d.path()).unwrap();
        let mut names: Vec<_> = p.entries.iter().map(|e| e.name.as_str()).collect();
        names.sort_unstable();
        assert_eq!(names, ["a.md", "b.yaml", "c.YML"]);
    }

    #[test]
    fn lista_html_e_htm() {
        let d = tempfile::tempdir().unwrap();
        for n in ["a.html", "b.HTM", "c.htm", "d.xhtml", "e.html.bak", "f.json"] {
            fs::write(d.path().join(n), "a").unwrap();
        }
        let p = Picker::scan(d.path()).unwrap();
        let mut names: Vec<_> = p.entries.iter().map(|e| e.name.as_str()).collect();
        names.sort_unstable();
        assert_eq!(names, ["a.html", "b.HTM", "c.htm"]);
    }

    #[cfg(unix)]
    #[test]
    fn nome_com_controle_e_saneado_mas_o_caminho_fica_intacto() {
        let d = tempfile::tempdir().unwrap();
        let nome = "a\u{1b}[31mb.md";
        fs::write(d.path().join(nome), "x").unwrap();
        let p = Picker::scan(d.path()).unwrap();
        assert_eq!(p.entries.len(), 1);
        assert!(!p.entries[0].name.contains('\u{1b}'), "{:?}", p.entries[0].name);
        assert!(p.entries[0].path.exists(), "o caminho real continua utilizável");
    }

    #[test]
    fn filtro_reordena_e_selecao_fica_no_limite() {
        let d = tempfile::tempdir().unwrap();
        for n in ["alfa.md", "beta.md", "alvo.md"] {
            fs::write(d.path().join(n), "a").unwrap();
        }
        let mut p = Picker::scan(d.path()).unwrap();
        p.push('a');
        p.push('l');
        assert_eq!(p.filtered.len(), 2);
        p.step(10);
        assert_eq!(p.selected, 1);
        p.step(-10);
        assert_eq!(p.selected, 0);
        p.pop();
        p.pop();
        assert_eq!(p.filtered.len(), 3);
    }

    #[test]
    fn idade_legivel() {
        let now = SystemTime::now();
        assert_eq!(age(Some(now), now), "agora");
        assert_eq!(age(Some(now - Duration::from_secs(300)), now), "5 min");
        assert_eq!(age(Some(now - Duration::from_secs(7200)), now), "2 h");
        assert_eq!(age(Some(now - Duration::from_secs(172_800)), now), "2 d");
        assert_eq!(age(None, now), "—");
    }
}
