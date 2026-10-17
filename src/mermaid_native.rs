//! Renderizador Mermaid nativo (sem Chromium): `mermaid-rs-renderer` gera o SVG e `resvg` o rasteriza.

#[cfg(feature = "mermaid")]
pub use real::*;

#[cfg(feature = "mermaid")]
mod real {
    use std::sync::{Arc, Mutex, OnceLock, PoisonError};

    use resvg::{tiny_skia, usvg};

    /// Tamanho máximo do fonte de um diagrama.
    pub const MAX_MERMAID_BYTES: usize = 64 * 1024;
    /// Mais linhas que isso é recusado antes de qualquer trabalho (o layout cresce com o grafo).
    pub const MAX_LINES: usize = 400;
    /// Mais conexões (`>` para setas, `&` para cada ponto extra) que isso é recusado: o custo do
    /// layout cresce mais que linearmente com as arestas (399 arestas levaram 44 s).
    pub const MAX_CONNECTIONS: usize = 100;
    /// Teto do SVG intermediário.
    pub const MAX_SVG_BYTES: usize = 2 * 1024 * 1024;
    /// Teto da imagem final: 1200×3000 px = 14,4 MB de RGBA, sempre.
    pub const MAX_WIDTH_PX: u32 = 1200;
    pub const MAX_HEIGHT_PX: u32 = 3000;
    /// Diagramas pequenos são ampliados até 2x (mais nitidez no `--graphics kitty`).
    const MAX_SCALE: f32 = 2.0;

    type Fonts = Arc<usvg::fontdb::Database>;

    /// Fontes do sistema, carregadas uma única vez (carregar custa dezenas de ms).
    fn system_fonts() -> &'static Fonts {
        static DB: OnceLock<Fonts> = OnceLock::new();
        DB.get_or_init(|| {
            let mut db = usvg::fontdb::Database::new();
            db.load_system_fonts();
            Arc::new(db)
        })
    }

    /// Há alguma fonte para desenhar o texto dos diagramas?
    #[cfg(test)]
    pub fn fonts_available() -> bool {
        !system_fonts().is_empty()
    }

    static PANIC_LOCK: Mutex<()> = Mutex::new(());

    thread_local! {
        /// Verdadeiro só na thread que está dentro de `guarded`: nela a mensagem de pânico fica muda.
        static SILENT: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    }

    static HOOK_ONCE: std::sync::Once = std::sync::Once::new();

    /// Instala (uma vez) um gancho que delega ao anterior, exceto na thread marcada em `SILENT`.
    /// Assim um pânico em outra thread (a da TUI, por exemplo) continua passando pelo gancho do
    /// `ratatui`, que restaura o terminal.
    fn install_hook() {
        HOOK_ONCE.call_once(|| {
            let prev = std::panic::take_hook();
            std::panic::set_hook(Box::new(move |info| {
                if !SILENT.with(std::cell::Cell::get) {
                    prev(info);
                }
            }));
        });
    }

    /// Roda `f` e transforma pânico em erro. Enquanto roda, a mensagem de pânico desta thread fica
    /// muda para não sujar a TUI; as chamadas continuam serializadas por `PANIC_LOCK`.
    fn guarded<T>(f: impl FnOnce() -> T) -> Result<T, String> {
        let _g = PANIC_LOCK.lock().unwrap_or_else(PoisonError::into_inner);
        install_hook();
        SILENT.with(|s| s.set(true));
        let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
        SILENT.with(|s| s.set(false));
        res.map_err(|_| "o renderizador nativo falhou neste diagrama".to_string())
    }

    /// O renderizador está compilado neste binário.
    pub fn available() -> bool {
        true
    }

    /// Desenha o diagrama Mermaid e devolve um PNG.
    pub fn render_png(source: &str) -> Result<Vec<u8>, String> {
        render_png_with(source, system_fonts())
    }

    /// Conta setas (`>`) e pontos extra de `&` (cada `&` multiplica as conexões).
    fn connections(source: &str) -> usize {
        source.matches('>').count() + source.matches('&').count()
    }

    fn render_png_with(source: &str, fonts: &Fonts) -> Result<Vec<u8>, String> {
        if source.len() > MAX_MERMAID_BYTES {
            return Err("diagrama grande demais".into());
        }
        if source.lines().count() > MAX_LINES {
            return Err(format!("diagrama com mais de {MAX_LINES} linhas"));
        }
        if connections(source) > MAX_CONNECTIONS {
            return Err(format!("diagrama complexo demais (mais de {MAX_CONNECTIONS} conexões)"));
        }
        guarded(|| {
            let svg = mermaid_rs_renderer::render(source)
                .map_err(|e| e.to_string().lines().next().unwrap_or("erro").to_string())?;
            // Depois do parse: erro de sintaxe é reportado mesmo sem fontes.
            if fonts.is_empty() {
                return Err("nenhuma fonte instalada no sistema para desenhar o texto".to_string());
            }
            rasterize(&svg, fonts)
        })?
    }

    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    fn rasterize(svg: &str, fonts: &Fonts) -> Result<Vec<u8>, String> {
        if svg.len() > MAX_SVG_BYTES {
            return Err("SVG gerado grande demais".into());
        }
        let opt = usvg::Options { fontdb: Arc::clone(fonts), ..Default::default() };
        let tree = usvg::Tree::from_str(svg, &opt).map_err(|e| e.to_string())?;
        let size = tree.size();
        let (w, h) = (size.width(), size.height());
        if !(w >= 1.0 && h >= 1.0) {
            return Err("diagrama sem tamanho".into());
        }
        let scale = (MAX_WIDTH_PX as f32 / w).min(MAX_HEIGHT_PX as f32 / h).min(MAX_SCALE);
        let (pw, ph) = ((w * scale).ceil() as u32, (h * scale).ceil() as u32);
        crate::mem::check().map_err(|e| e.to_string())?;
        let mut pixmap = tiny_skia::Pixmap::new(pw.max(1), ph.max(1)).ok_or("não consegui alocar a imagem")?;
        resvg::render(&tree, tiny_skia::Transform::from_scale(scale, scale), &mut pixmap.as_mut());
        pixmap.encode_png().map_err(|e| e.to_string())
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        const FLOW: &str = "flowchart LR\n  A[Início] --> B{Decide?}\n  B -->|sim| C[Faz]\n  B -->|não| D[Para]\n";

        fn dims(png: &[u8]) -> (u32, u32) {
            let img = image::load_from_memory(png).expect("PNG válido");
            (img.width(), img.height())
        }

        #[test]
        fn fluxograma_vira_png() {
            let png = render_png(FLOW).expect("deveria renderizar");
            assert_eq!(&png[1..4], b"PNG");
            let (w, h) = dims(&png);
            assert!(w > 50 && h > 20, "{w}x{h}");
        }

        /// Regressão da troca de `resvg`/`usvg`/`fontdb`: vários tipos de diagrama, com texto, viram PNG não vazio.
        #[test]
        fn tipos_de_diagrama_variados_viram_png_com_conteudo() {
            if sem_fontes() {
                return;
            }
            let casos = [
                FLOW,
                "sequenceDiagram\n  Ana->>Beto: Oi\n  Beto-->>Ana: Olá\n",
                "pie title Uso\n  \"A\" : 40\n  \"B\" : 60\n",
                "stateDiagram-v2\n  [*] --> Ativo\n  Ativo --> [*]\n",
            ];
            for src in casos {
                let png = render_png(src).unwrap_or_else(|e| panic!("{src:?}: {e}"));
                let img = image::load_from_memory(&png).expect("PNG válido").to_rgba8();
                let cores: std::collections::HashSet<[u8; 4]> = img.pixels().map(|p| p.0).take(200_000).collect();
                assert!(cores.len() > 3, "imagem sem conteúdo para {src:?}");
            }
        }

        /// O texto precisa ser desenhado (fontes do sistema carregadas pelo `fontdb`): rótulos diferentes mudam os pixels.
        #[test]
        fn rotulos_diferentes_geram_pixels_diferentes() {
            if sem_fontes() {
                return;
            }
            let a = render_png("flowchart LR\n  A[Alfa] --> B[Beta]\n").unwrap();
            let b = render_png("flowchart LR\n  A[Zzzz] --> B[Yyyy]\n").unwrap();
            assert_ne!(a, b);
        }

        #[test]
        fn sintaxe_invalida_vira_erro_em_portugues_ou_do_crate() {
            let e = render_png("isto não é mermaid").unwrap_err();
            assert!(!e.trim().is_empty());
        }

        /// Máquinas sem fontes não conseguem desenhar texto: os testes de imagem são pulados.
        fn sem_fontes() -> bool {
            !fonts_available()
        }

        #[test]
        fn fonte_grande_demais_e_recusada() {
            let src = "A".repeat(MAX_MERMAID_BYTES + 1);
            assert!(render_png(&src).unwrap_err().contains("grande"));
        }

        #[test]
        fn diagrama_com_linhas_demais_e_recusado() {
            let src = format!("graph TD\n{}", "A-->B\n".repeat(MAX_LINES + 1));
            let e = render_png(&src).unwrap_err();
            assert!(e.contains("linhas"), "{e}");
        }

        #[test]
        fn diagrama_com_conexoes_demais_e_recusado_sem_rodar_o_layout() {
            let src = format!(
                "flowchart LR\n{}",
                (0..=MAX_CONNECTIONS).map(|i| format!("N{i}-->N{}\n", i + 1)).collect::<String>()
            );
            let t = std::time::Instant::now();
            let e = render_png(&src).unwrap_err();
            assert!(e.contains("conexões"), "{e}");
            assert!(t.elapsed() < std::time::Duration::from_secs(1), "{:?}", t.elapsed());
        }

        #[test]
        fn conexoes_em_uma_linha_so_tambem_contam() {
            let src = format!(
                "flowchart LR\n{}Z",
                (0..=MAX_CONNECTIONS).map(|i| format!("N{i}-->")).collect::<String>()
            );
            let e = render_png(&src).unwrap_err();
            assert!(e.contains("conexões"), "{e}");
        }

        #[test]
        fn ecomercial_conta_como_conexao() {
            let src = format!(
                "flowchart LR\n{} --> Z\n",
                (0..=MAX_CONNECTIONS).map(|i| format!("N{i}")).collect::<Vec<_>>().join(" & ")
            );
            let e = render_png(&src).unwrap_err();
            assert!(e.contains("conexões"), "{e}");
        }

        #[test]
        fn diagrama_normal_continua_dentro_do_teto() {
            // Pode falhar por outro motivo (sem fontes); o que não pode é ser o teto de conexões.
            if let Err(e) = render_png(FLOW) {
                assert!(!e.contains("conexões"), "{e}");
            }
        }

        #[test]
        fn sem_fontes_o_erro_e_claro() {
            let vazio: Fonts = Arc::new(usvg::fontdb::Database::new());
            let e = render_png_with("graph TD\n  A --> B\n", &vazio).unwrap_err();
            assert!(e.contains("fonte"), "{e}");
        }

        #[test]
        fn sintaxe_invalida_nao_depende_de_fontes() {
            let vazio: Fonts = Arc::new(usvg::fontdb::Database::new());
            let e = render_png_with("isto não é mermaid", &vazio).unwrap_err();
            assert!(!e.contains("fonte instalada"), "{e}");
        }

        #[test]
        fn panico_do_renderizador_vira_erro() {
            let r: Result<(), String> = guarded(|| panic!("boom"));
            assert!(r.unwrap_err().contains("falhou"));
            // e a próxima chamada continua funcionando
            assert_eq!(guarded(|| 7).unwrap(), 7);
        }

        #[test]
        fn diagrama_largo_e_reduzido_para_os_tetos() {
            if sem_fontes() {
                return;
            }
            let mut src = String::from("flowchart LR\n");
            for i in 0..40 {
                src.push_str(&format!("N{i}[no {i}] --> N{}\n", i + 1));
            }
            let (w, h) = dims(&render_png(&src).unwrap());
            assert!(w <= MAX_WIDTH_PX && h <= MAX_HEIGHT_PX, "{w}x{h}");
            assert!(w > 0 && h > 0);
        }

        #[test]
        fn diagrama_pequeno_e_ampliado_ate_2x() {
            if sem_fontes() {
                return;
            }
            // 93×156 no tamanho natural; com 2x fica com ~187×313.
            let (w, _) = dims(&render_png("graph TD\n A-->B\n").unwrap());
            assert!(w >= 170, "largura {w}");
        }

        #[test]
        fn rotulos_com_acentos_cjk_e_emoji_nao_quebram() {
            if sem_fontes() {
                return;
            }
            let png = render_png("flowchart TD\n  A[Ação ✓ 日本語 😀] --> B[Fim]\n").unwrap();
            assert_eq!(&png[1..4], b"PNG");
        }

        #[test]
        fn bloco_incompleto_vira_erro_e_nao_panico() {
            let e = render_png("flowchart TD\n  A --> ").unwrap_err();
            assert!(!e.trim().is_empty());
        }

        #[test]
        fn guarded_transforma_panico_em_erro() {
            let r: Result<(), String> = guarded(|| panic!("boom"));
            assert!(r.is_err());
            assert_eq!(guarded(|| 7).unwrap(), 7);
        }

        #[test]
        fn guarded_nao_deixa_a_thread_muda_depois_de_sair() {
            let _ = guarded(|| panic!("boom"));
            assert!(!SILENT.with(std::cell::Cell::get), "a flag precisa voltar a false");
        }
    }
}

/// Sem a feature `mermaid` não há renderizador: o diagrama fica como código.
#[cfg(not(feature = "mermaid"))]
pub fn available() -> bool {
    false
}

#[cfg(not(feature = "mermaid"))]
pub fn render_png(_source: &str) -> Result<Vec<u8>, String> {
    Err("mermaid não foi compilado neste binário; recompile com --features mermaid".into())
}

#[cfg(not(feature = "mermaid"))]
#[cfg(test)]
pub fn fonts_available() -> bool {
    false
}
