//! Markdown → `Document`. Usa as mesmas opções do `pulldown-cmark` do resto do programa
//! (`render::parse_options`) e o mesmo saneamento de texto (`render::sanitize`).

use super::model::{Block, Document, Inline, ListItem, Run};
use super::Budget;
use mdv_text::{parse_options, sanitize, summary_text};
use pulldown_cmark::{Alignment, CodeBlockKind, Event, Parser, Tag, TagEnd};

/// Profundidade máxima de blocos aninhados (listas dentro de citações dentro de listas...).
pub const MAX_DEPTH: usize = 32;
/// Teto de blocos no documento.
pub const MAX_BLOCKS: usize = 200_000;
/// Teto de eventos do `pulldown-cmark` (cada um vira no máximo um trecho de texto).
pub const MAX_EVENTS: usize = 400_000;
/// Teto de colunas de uma tabela.
pub const MAX_TABLE_COLS: usize = 100;

/// Contêiner aberto enquanto os eventos chegam.
enum Frame {
    Root(Vec<Block>),
    Quote { title: Option<String>, blocks: Vec<Block> },
    List { ordered: bool, start: u64, items: Vec<ListItem> },
    Item { task: Option<bool>, blocks: Vec<Block> },
}

#[derive(Default)]
struct TableBuilder {
    aligns: Vec<Alignment>,
    header: Vec<Vec<Inline>>,
    rows: Vec<Vec<Vec<Inline>>>,
    row: Vec<Vec<Inline>>,
}

struct Builder {
    stack: Vec<Frame>,
    inline: Vec<Inline>,
    bold: u32,
    italic: u32,
    strike: u32,
    link: Option<String>,
    heading: Option<u8>,
    /// Bloco de código aberto: (linguagem, texto).
    code: Option<(String, String)>,
    /// Imagem aberta: (origem, texto alternativo).
    image: Option<(String, String)>,
    table: Option<TableBuilder>,
    /// Texto do front matter, enquanto aberto.
    meta: Option<String>,
    title: Option<String>,
    blocks_made: usize,
    events: usize,
    max_events: usize,
}

/// Converte `md` no modelo; falha se passar dos tetos de memória, profundidade ou blocos.
pub fn parse(md: &str, budget: &dyn Budget) -> Result<Document, String> {
    parse_with_limit(md, MAX_EVENTS, budget)
}

/// Como `parse`, com teto de eventos escolhido (os testes usam um teto pequeno).
fn parse_with_limit(md: &str, max_events: usize, budget: &dyn Budget) -> Result<Document, String> {
    budget.check_parse(md.len())?;
    let mut b = Builder::new();
    b.max_events = max_events;
    for ev in Parser::new_ext(md, parse_options()) {
        budget.check()?;
        b.event(ev)?;
    }
    b.finish()
}

impl Builder {
    fn new() -> Self {
        Builder {
            stack: vec![Frame::Root(Vec::new())],
            inline: Vec::new(),
            bold: 0,
            italic: 0,
            strike: 0,
            link: None,
            heading: None,
            code: None,
            image: None,
            table: None,
            meta: None,
            title: None,
            blocks_made: 0,
            events: 0,
            max_events: MAX_EVENTS,
        }
    }

    fn finish(mut self) -> Result<Document, String> {
        self.flush_paragraph()?;
        match self.stack.pop() {
            Some(Frame::Root(blocks)) if self.stack.is_empty() => Ok(Document { title: self.title, blocks }),
            _ => Err("Markdown mal formado (contêiner aberto)".to_string()),
        }
    }

    /// Entrega um bloco ao contêiner aberto mais interno.
    fn push_block(&mut self, block: Block) -> Result<(), String> {
        self.blocks_made += 1;
        if self.blocks_made > MAX_BLOCKS {
            return Err(format!("documento com blocos demais (limite {MAX_BLOCKS})"));
        }
        match self.stack.last_mut() {
            Some(Frame::Root(blocks) | Frame::Quote { blocks, .. } | Frame::Item { blocks, .. }) => {
                blocks.push(block);
                Ok(())
            }
            _ => Err("bloco fora de lugar".to_string()),
        }
    }

    fn open(&mut self, frame: Frame) -> Result<(), String> {
        if self.stack.len() >= MAX_DEPTH {
            return Err(format!("aninhamento fundo demais (limite {MAX_DEPTH})"));
        }
        self.stack.push(frame);
        Ok(())
    }

    /// Texto em linha acumulado vira parágrafo (se houver algo além de espaços).
    fn flush_paragraph(&mut self) -> Result<(), String> {
        let inline = std::mem::take(&mut self.inline);
        if inline.iter().any(|i| matches!(i, Inline::Break) || matches!(i, Inline::Run(r) if !r.text.trim().is_empty())) {
            self.push_block(Block::Paragraph(inline))?;
        }
        Ok(())
    }

    fn add_text(&mut self, text: &str, code: bool) {
        if let Some((_, alt)) = &mut self.image {
            alt.push_str(text);
            return;
        }
        if text.is_empty() {
            return;
        }
        self.inline.push(Inline::Run(Run {
            text: text.to_string(),
            bold: self.bold > 0,
            italic: self.italic > 0,
            strike: self.strike > 0,
            code,
            link: self.link.clone(),
        }));
    }

    fn event(&mut self, ev: Event<'_>) -> Result<(), String> {
        self.events += 1;
        if self.events > self.max_events {
            return Err(format!("documento grande demais para exportar (limite de {} elementos)", self.max_events));
        }
        match ev {
            Event::Start(tag) => self.start(tag),
            Event::End(tag) => self.end(tag),
            Event::Text(t) => {
                let t = sanitize(&t);
                if let Some((_, buf)) = &mut self.code {
                    buf.push_str(&t);
                } else if let Some(buf) = &mut self.meta {
                    buf.push_str(&t);
                } else {
                    self.add_text(&t, false);
                }
                Ok(())
            }
            Event::Code(t) => {
                self.add_text(&sanitize(&t), true);
                Ok(())
            }
            Event::SoftBreak => {
                self.add_text(" ", false);
                Ok(())
            }
            Event::HardBreak => {
                self.inline.push(Inline::Break);
                Ok(())
            }
            Event::Rule => {
                self.flush_paragraph()?;
                self.push_block(Block::Rule)
            }
            Event::TaskListMarker(done) => {
                if let Some(Frame::Item { task, .. }) = self.stack.last_mut() {
                    *task = Some(done);
                }
                Ok(())
            }
            Event::FootnoteReference(label) => {
                self.add_text(&format!("[^{}]", sanitize(&label)), false);
                Ok(())
            }
            Event::Html(h) => self.html(&h),
            // HTML em linha (`<br>`, `<b>`...) não vira nada; o texto em volta segue normal.
            _ => Ok(()),
        }
    }

    /// Só `<summary>` (vira parágrafo em negrito) é reconhecido.
    fn html(&mut self, html: &str) -> Result<(), String> {
        self.flush_paragraph()?;
        for line in html.lines() {
            if let Some(s) = summary_text(line) {
                self.push_block(Block::Paragraph(vec![Inline::Run(Run::bold(sanitize(s).into_owned()))]))?;
            }
        }
        Ok(())
    }

    fn start(&mut self, tag: Tag<'_>) -> Result<(), String> {
        match tag {
            Tag::Paragraph => {}
            Tag::Heading { level, .. } => {
                self.flush_paragraph()?;
                self.heading = Some(level as u8);
            }
            Tag::BlockQuote(kind) => {
                self.flush_paragraph()?;
                self.open(Frame::Quote { title: kind.map(|k| alert_title(k).to_string()), blocks: Vec::new() })?;
            }
            Tag::CodeBlock(kind) => {
                self.flush_paragraph()?;
                let lang = match kind {
                    CodeBlockKind::Fenced(info) => info.split_whitespace().next().unwrap_or("").to_string(),
                    CodeBlockKind::Indented => String::new(),
                };
                self.code = Some((sanitize(&lang).into_owned(), String::new()));
            }
            Tag::List(first) => {
                self.flush_paragraph()?;
                self.open(Frame::List { ordered: first.is_some(), start: first.unwrap_or(1), items: Vec::new() })?;
            }
            Tag::Item => self.open(Frame::Item { task: None, blocks: Vec::new() })?,
            Tag::Emphasis => self.italic += 1,
            Tag::Strong => self.bold += 1,
            Tag::Strikethrough => self.strike += 1,
            Tag::Link { dest_url, .. } => self.link = Some(sanitize(&dest_url).into_owned()),
            Tag::Image { dest_url, .. } => {
                self.flush_paragraph()?;
                self.image = Some((sanitize(&dest_url).into_owned(), String::new()));
            }
            Tag::Table(aligns) => {
                self.flush_paragraph()?;
                self.table = Some(TableBuilder { aligns, ..TableBuilder::default() });
            }
            Tag::TableHead | Tag::TableRow => {}
            Tag::TableCell => self.inline.clear(),
            Tag::FootnoteDefinition(label) => {
                self.flush_paragraph()?;
                self.add_text(&format!("[^{}]: ", sanitize(&label)), false);
            }
            Tag::MetadataBlock(_) => self.meta = Some(String::new()),
            // Tags que o modelo não distingue (HTML em bloco, listas de definição...): o conteúdo segue.
            _ => {}
        }
        Ok(())
    }

    fn end(&mut self, tag: TagEnd) -> Result<(), String> {
        match tag {
            TagEnd::Paragraph => self.flush_paragraph()?,
            TagEnd::Heading(_) => {
                let level = self.heading.take().unwrap_or(1);
                let text = std::mem::take(&mut self.inline);
                self.push_block(Block::Heading { level, text })?;
            }
            TagEnd::BlockQuote(_) => {
                self.flush_paragraph()?;
                if let Some(Frame::Quote { title, blocks }) = self.stack.pop() {
                    self.push_block(Block::Quote { title, blocks })?;
                }
            }
            TagEnd::CodeBlock => {
                if let Some((lang, mut text)) = self.code.take() {
                    if text.ends_with('\n') {
                        text.pop();
                    }
                    self.push_block(Block::Code { lang, text })?;
                }
            }
            TagEnd::List(_) => {
                if let Some(Frame::List { ordered, start, items }) = self.stack.pop() {
                    self.push_block(Block::List { ordered, start, items })?;
                }
            }
            TagEnd::Item => {
                self.flush_paragraph()?;
                if let Some(Frame::Item { task, blocks }) = self.stack.pop() {
                    if let Some(Frame::List { items, .. }) = self.stack.last_mut() {
                        items.push(ListItem { task, blocks });
                    }
                }
            }
            TagEnd::Emphasis => self.italic = self.italic.saturating_sub(1),
            TagEnd::Strong => self.bold = self.bold.saturating_sub(1),
            TagEnd::Strikethrough => self.strike = self.strike.saturating_sub(1),
            TagEnd::Link => self.link = None,
            TagEnd::Image => {
                if let Some((src, alt)) = self.image.take() {
                    self.push_block(Block::Image { alt, src })?;
                }
            }
            TagEnd::TableCell => {
                let cell = std::mem::take(&mut self.inline);
                if let Some(t) = &mut self.table {
                    if t.row.len() >= MAX_TABLE_COLS {
                        return Err(format!("tabela com colunas demais (limite de {MAX_TABLE_COLS})"));
                    }
                    t.row.push(cell);
                }
            }
            TagEnd::TableHead => {
                if let Some(t) = &mut self.table {
                    t.header = std::mem::take(&mut t.row);
                }
            }
            TagEnd::TableRow => {
                if let Some(t) = &mut self.table {
                    let row = std::mem::take(&mut t.row);
                    t.rows.push(row);
                }
            }
            TagEnd::Table => {
                if let Some(t) = self.table.take() {
                    self.push_block(Block::Table { aligns: t.aligns, header: t.header, rows: t.rows })?;
                }
            }
            TagEnd::FootnoteDefinition => self.flush_paragraph()?,
            TagEnd::MetadataBlock(_) => {
                if let Some(text) = self.meta.take() {
                    self.title = front_matter_title(&text);
                }
            }
            _ => {}
        }
        Ok(())
    }
}

/// Valor de `title:` no front matter YAML (sem aspas); `None` se não houver.
/// Rótulo de um alerta GFM (os mesmos textos do renderizador do terminal, sem emoji).
fn alert_title(kind: pulldown_cmark::BlockQuoteKind) -> &'static str {
    use pulldown_cmark::BlockQuoteKind as K;
    match kind {
        K::Note => "Nota",
        K::Tip => "Dica",
        K::Important => "Importante",
        K::Warning => "Aviso",
        K::Caution => "Cuidado",
    }
}

fn front_matter_title(yaml: &str) -> Option<String> {
    yaml.lines().find_map(|l| {
        let v = l.strip_prefix("title:")?.trim().trim_matches(['"', '\'']).trim();
        (!v.is_empty()).then(|| v.to_string())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::plain_text;

    fn doc(md: &str) -> Document {
        parse(md, &crate::Unlimited).expect("parse")
    }

    #[test]
    fn titulo_paragrafo_e_estilos() {
        let d = doc("# Olá\n\ntexto **forte** e *itálico* e `cod` e [link](http://a.b)\n");
        assert!(matches!(&d.blocks[0], Block::Heading { level: 1, text } if plain_text(text) == "Olá"));
        let Block::Paragraph(p) = &d.blocks[1] else { panic!("{:?}", d.blocks[1]) };
        let runs: Vec<&Run> = p.iter().filter_map(|i| if let Inline::Run(r) = i { Some(r) } else { None }).collect();
        assert!(runs.iter().any(|r| r.text == "forte" && r.bold));
        assert!(runs.iter().any(|r| r.text == "itálico" && r.italic));
        assert!(runs.iter().any(|r| r.text == "cod" && r.code));
        assert!(runs.iter().any(|r| r.text == "link" && r.link.as_deref() == Some("http://a.b")));
    }

    #[test]
    fn lista_aninhada_com_tarefa() {
        let d = doc("- [x] feito\n- [ ] falta\n  - filho\n");
        let Block::List { ordered, items, .. } = &d.blocks[0] else { panic!() };
        assert!(!ordered);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].task, Some(true));
        assert_eq!(items[1].task, Some(false));
        assert!(items[1].blocks.iter().any(|b| matches!(b, Block::List { .. })), "sublista");
    }

    #[test]
    fn lista_ordenada_guarda_o_inicio() {
        let d = doc("3. três\n4. quatro\n");
        assert!(matches!(&d.blocks[0], Block::List { ordered: true, start: 3, .. }));
    }

    #[test]
    fn alerta_vira_citacao_com_titulo() {
        let d = doc("> [!WARNING]\n> cuidado\n");
        assert!(matches!(&d.blocks[0], Block::Quote { title: Some(t), .. } if t == "Aviso"));
    }

    #[test]
    fn codigo_tabela_regua_e_imagem() {
        let d = doc("```rust\nfn a() {}\n```\n\n| a | b |\n|---|:-:|\n| 1 | 2 |\n\n---\n\n![alt](x.png)\n");
        assert!(matches!(&d.blocks[0], Block::Code { lang, text } if lang == "rust" && text == "fn a() {}"));
        let Block::Table { header, rows, aligns } = &d.blocks[1] else { panic!("{:?}", d.blocks[1]) };
        assert_eq!(header.len(), 2);
        assert_eq!(rows.len(), 1);
        assert_eq!(aligns.len(), 2);
        assert!(matches!(&d.blocks[2], Block::Rule));
        assert!(matches!(&d.blocks[3], Block::Image { alt, src } if alt == "alt" && src == "x.png"));
    }

    #[test]
    fn front_matter_da_o_titulo_e_nao_vira_bloco() {
        let d = doc("---\ntitle: \"Meu plano\"\ntags: [a]\n---\n\n# H\n");
        assert_eq!(d.title.as_deref(), Some("Meu plano"));
        assert_eq!(d.blocks.len(), 1);
    }

    #[test]
    fn summary_vira_negrito_e_details_some() {
        let d = doc("<details>\n<summary>Ver mais</summary>\n\ncorpo\n\n</details>\n");
        let texts: Vec<String> = d
            .blocks
            .iter()
            .filter_map(|b| if let Block::Paragraph(p) = b { Some(plain_text(p)) } else { None })
            .collect();
        assert!(texts.contains(&"Ver mais".to_string()), "{texts:?}");
        assert!(texts.contains(&"corpo".to_string()), "{texts:?}");
        assert!(!texts.iter().any(|t| t.contains("details")), "{texts:?}");
    }

    #[test]
    fn controles_sao_saneados() {
        let d = doc("a\u{1b}[31mb\n");
        let Block::Paragraph(p) = &d.blocks[0] else { panic!() };
        assert!(!plain_text(p).contains('\u{1b}'));
    }

    #[test]
    fn aninhamento_fundo_demais_e_recusado() {
        let md = "> ".repeat(MAX_DEPTH + 5) + "x\n";
        assert!(parse(&md, &crate::Unlimited).unwrap_err().contains("aninhamento"));
    }

    #[test]
    fn reguas_repetidas_funcionam() {
        let d = doc(&"---\n\n".repeat(10));
        assert_eq!(d.blocks.iter().filter(|b| matches!(b, Block::Rule)).count(), 10);
    }

    #[test]
    fn elementos_demais_sao_recusados() {
        let err = parse_with_limit(&"a\n\n".repeat(10), 5, &crate::Unlimited).unwrap_err();
        assert!(err.contains("limite de 5 elementos"), "{err}");
        assert!(parse_with_limit("a\n", 100, &crate::Unlimited).is_ok());
    }

    #[test]
    fn tabela_com_colunas_demais_e_recusada() {
        let cols = MAX_TABLE_COLS + 1;
        let head = format!("|{}\n", " a |".repeat(cols));
        let sep = format!("|{}\n", "---|".repeat(cols));
        let err = parse(&format!("{head}{sep}"), &crate::Unlimited).unwrap_err();
        assert!(err.contains("colunas demais"), "{err}");
        let ok = MAX_TABLE_COLS;
        let head = format!("|{}\n", " a |".repeat(ok));
        let sep = format!("|{}\n", "---|".repeat(ok));
        assert!(parse(&format!("{head}{sep}"), &crate::Unlimited).is_ok());
    }

    #[test]
    fn budget_esgotado_aborta_com_erro_e_sem_panico() {
        let e = parse("# a\n\ntexto\n", &crate::testutil::Exhausted).unwrap_err();
        assert!(e.contains("memória"), "{e}");
    }
}
