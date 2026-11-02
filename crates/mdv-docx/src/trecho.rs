//! Conteúdo de um run (`w:r`): texto, quebras, referências de nota e imagens.
//! Só lê o run; quem agrupa os runs em parágrafos e decide o link fica no módulo de parágrafo.

use crate::contexto::Contexto;
use crate::estilo_run::ler_rpr;
use crate::modelo::{chave_nota, Marcas, TipoNota, Trecho};
use crate::xml_leitor::{Evento, Leitor, Tag};
use mdv_text::sanitize;
use std::rc::Rc;

/// Lê um `w:r` (chamada logo depois do `Abre("r")`) e consome até o `Fecha` dele.
pub(crate) fn ler_run(
    l: &mut Leitor,
    ctx: &Contexto,
    link: Option<&Rc<str>>,
    saida: &mut Vec<Trecho>,
) -> Result<(), String> {
    let mut run = Run::novo(link, saida);
    while let Some(evento) = l.dentro("r")? {
        if let Evento::Abre(tag) = evento {
            run.filho(l, ctx, &tag)?;
        }
    }
    run.encerrar();
    Ok(())
}

/// Estado da leitura de um run: o texto ainda não emitido e as marcas do `rPr`.
struct Run<'a> {
    link: Option<&'a Rc<str>>,
    saida: &'a mut Vec<Trecho>,
    /// Texto acumulado de `t`, `tab` e `noBreakHyphen` até uma quebra, nota, desenho ou o fim.
    pendente: String,
    marcas: Marcas,
    /// O `w:rPr` pode vir depois do texto no XML: os trechos saem com marcas padrão e recebem as
    /// do run no fim, a partir deste índice de `saida`.
    inicio: usize,
}

impl<'a> Run<'a> {
    fn novo(link: Option<&'a Rc<str>>, saida: &'a mut Vec<Trecho>) -> Self {
        let inicio = saida.len();
        Run { link, saida, pendente: String::new(), marcas: Marcas::default(), inicio }
    }

    /// Trata um filho direto do run (ou um descendente de elemento transparente).
    fn filho(&mut self, l: &mut Leitor, ctx: &Contexto, tag: &Tag) -> Result<(), String> {
        match tag.nome.as_str() {
            "rPr" => self.marcas = ler_rpr(l, ctx)?,
            // Só `w:t` é texto do documento; `delText` e `instrText` (abaixo) são pulados.
            "t" => ler_t(l, &mut self.pendente)?,
            "tab" => self.pendente.push(' '),
            "noBreakHyphen" => self.pendente.push('-'),
            "br" => self.quebra(tag),
            "footnoteReference" | "endnoteReference" => self.nota(tag),
            // Desenho é consumido inteiro só para ler o `descr`; o que há dentro (caixas de
            // texto, formas) é descartado, pois a conversão não trata caixas de texto.
            "drawing" | "pict" => self.desenho(l)?,
            // O Word repete o desenho em `Fallback` para versões antigas; só vale o `Choice`.
            "object" | "delText" | "instrText" | "Fallback" => l.pular()?,
            _ => {}
        }
        Ok(())
    }

    /// Quebra de página ou de coluna não é quebra de linha no Markdown.
    fn quebra(&mut self, tag: &Tag) {
        if matches!(tag.attr("type"), None | Some("textWrapping")) {
            self.esvaziar();
            self.saida.push(Trecho::Quebra);
        }
    }

    fn nota(&mut self, tag: &Tag) {
        self.esvaziar();
        let tipo = TipoNota::da_referencia(&tag.nome);
        self.saida.push(Trecho::NotaRef(chave_nota(tipo, tag.attr("id").unwrap_or(""))));
    }

    fn desenho(&mut self, l: &mut Leitor) -> Result<(), String> {
        self.esvaziar();
        let descr = ler_desenho(l)?;
        self.saida.push(Trecho::Imagem(sanitize(&descr).into_owned()));
        Ok(())
    }

    /// Passa o texto acumulado (já sem controles de terminal) para a saída; vazio não gera trecho.
    fn esvaziar(&mut self) {
        if self.pendente.is_empty() {
            return;
        }
        let texto = sanitize(&self.pendente).into_owned();
        self.pendente.clear();
        self.saida.push(Trecho::Texto { texto, marcas: Marcas::default(), link: self.link.cloned() });
    }

    /// Emite o que sobrou e aplica as marcas do `rPr` aos textos deste run.
    fn encerrar(mut self) {
        self.esvaziar();
        for t in &mut self.saida[self.inicio..] {
            if let Trecho::Texto { marcas, .. } = t {
                *marcas = self.marcas;
            }
        }
    }
}

/// Lê o texto de um `w:t` (já aberto) até o `Fecha` dele, juntando em `pendente`.
fn ler_t(l: &mut Leitor, pendente: &mut String) -> Result<(), String> {
    while let Some(evento) = l.dentro("t")? {
        if let Evento::Texto(t) = evento {
            pendente.push_str(&t);
        }
    }
    Ok(())
}

/// Consome um `drawing`/`pict` (já aberto) e devolve o `descr` do primeiro `docPr`, ou vazio.
/// Percorre a subárvore evento a evento contando a profundidade, sem sair do desenho.
fn ler_desenho(l: &mut Leitor) -> Result<String, String> {
    let mut descr: Option<String> = None;
    let mut visto = false;
    let mut profundidade = 1usize;
    while profundidade > 0 {
        match l.proximo()? {
            None => return Err("fim inesperado do documento".into()),
            Some(Evento::Abre(tag)) => {
                profundidade += 1;
                if tag.nome == "docPr" && !visto {
                    visto = true;
                    descr = tag.attr("descr").map(str::to_string);
                }
            }
            Some(Evento::Fecha(_)) => profundidade -= 1,
            Some(Evento::Texto(_)) => {}
        }
    }
    Ok(descr.unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contexto::com_contexto;
    use crate::modelo::Marcas;
    use crate::Unlimited;

    fn run_com_link(corpo: &str, link: Option<&str>) -> Vec<Trecho> {
        let xml = format!(
            r#"<w:r xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing" xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006">{corpo}</w:r>"#
        );
        com_contexto("", "", |ctx| {
            let mut l = Leitor::novo(xml.as_bytes(), &Unlimited);
            l.ate("r").unwrap().unwrap();
            let mut saida = Vec::new();
            ler_run(&mut l, ctx, link.map(Rc::from).as_ref(), &mut saida).unwrap();
            assert_eq!(l.proximo().unwrap(), None, "o run deve ser consumido por inteiro");
            saida
        })
    }

    fn run(corpo: &str) -> Vec<Trecho> {
        run_com_link(corpo, None)
    }

    fn texto(s: &str) -> Trecho {
        Trecho::Texto { texto: s.into(), marcas: Marcas::default(), link: None }
    }

    #[test]
    fn texto_simples() {
        assert_eq!(run("<w:t>oi</w:t>"), vec![texto("oi")]);
    }

    #[test]
    fn varios_t_no_mesmo_run_se_juntam() {
        assert_eq!(run("<w:t>a</w:t><w:t>b</w:t>"), vec![texto("ab")]);
    }

    #[test]
    fn tab_vira_espaco() {
        assert_eq!(run("<w:t>a</w:t><w:tab/><w:t>b</w:t>"), vec![texto("a b")]);
    }

    #[test]
    fn br_vira_quebra() {
        assert_eq!(run("<w:br/>"), vec![Trecho::Quebra]);
        assert_eq!(run(r#"<w:br w:type="textWrapping"/>"#), vec![Trecho::Quebra]);
        assert_eq!(run(r#"<w:br w:type="page"/>"#), vec![]);
        assert_eq!(run(r#"<w:br w:type="column"/>"#), vec![]);
    }

    #[test]
    fn quebra_separa_o_texto_na_ordem() {
        assert_eq!(
            run("<w:t>a</w:t><w:br/><w:t>b</w:t>"),
            vec![texto("a"), Trecho::Quebra, texto("b")]
        );
    }

    #[test]
    fn hifen_nao_separavel() {
        assert_eq!(run("<w:noBreakHyphen/>"), vec![texto("-")]);
    }

    #[test]
    fn texto_apagado_dentro_do_run_e_ignorado() {
        assert_eq!(run("<w:delText>x</w:delText>"), vec![]);
        assert_eq!(run("<w:instrText>HYPERLINK</w:instrText>"), vec![]);
    }

    #[test]
    fn texto_solto_fora_de_t_e_ignorado() {
        assert_eq!(run("solto<w:t>a</w:t>"), vec![texto("a")]);
    }

    #[test]
    fn t_vazio_nao_gera_trecho() {
        assert_eq!(run("<w:t/>"), vec![]);
    }

    #[test]
    fn nota_de_rodape_vira_referencia() {
        assert_eq!(run(r#"<w:footnoteReference w:id="2"/>"#), vec![Trecho::NotaRef("n2".into())]);
        assert_eq!(run(r#"<w:endnoteReference w:id="2"/>"#), vec![Trecho::NotaRef("e2".into())]);
    }

    #[test]
    fn nota_separa_o_texto_na_ordem() {
        assert_eq!(
            run(r#"<w:t>a</w:t><w:footnoteReference w:id="1"/><w:t>b</w:t>"#),
            vec![texto("a"), Trecho::NotaRef("n1".into()), texto("b")]
        );
    }

    #[test]
    fn imagem_usa_a_descricao() {
        let com = r#"<w:drawing><wp:inline><wp:docPr id="1" name="Figura 1" descr="gráfico de vendas"/></wp:inline></w:drawing>"#;
        assert_eq!(run(com), vec![Trecho::Imagem("gráfico de vendas".into())]);
        let sem = r#"<w:drawing><wp:inline><wp:docPr id="1" name="Figura 1"/></wp:inline></w:drawing>"#;
        assert_eq!(run(sem), vec![Trecho::Imagem(String::new())]);
        assert_eq!(run("<w:pict><w:p/></w:pict>"), vec![Trecho::Imagem(String::new())]);
    }

    #[test]
    fn imagem_fica_entre_os_textos() {
        let corpo = r#"<w:t>a</w:t><w:drawing><wp:inline><wp:docPr id="1" name="x" descr="d"/></wp:inline></w:drawing><w:t>b</w:t>"#;
        assert_eq!(run(corpo), vec![texto("a"), Trecho::Imagem("d".into()), texto("b")]);
    }

    #[test]
    fn caixa_de_texto_dentro_do_desenho_e_ignorada() {
        let corpo = r#"<w:drawing><wp:anchor><wp:docPr id="1" name="x" descr="caixa"/><w:txbxContent><w:p><w:r><w:t>segredo</w:t></w:r></w:p></w:txbxContent></wp:anchor></w:drawing>"#;
        assert_eq!(run(corpo), vec![Trecho::Imagem("caixa".into())]);
    }

    #[test]
    fn so_o_primeiro_docpr_conta() {
        let corpo = r#"<w:drawing><wp:docPr id="1" name="x"/><wp:docPr id="2" name="y" descr="outro"/></w:drawing><w:t>fim</w:t>"#;
        assert_eq!(run(corpo), vec![Trecho::Imagem(String::new()), texto("fim")]);
    }

    #[test]
    fn descricao_da_imagem_perde_controle_de_terminal() {
        let corpo = "<w:drawing><wp:docPr id=\"1\" name=\"x\" descr=\"a\u{1b}[31mb\"/></w:drawing>";
        match &run(corpo)[..] {
            [Trecho::Imagem(d)] => assert!(!d.contains('\u{1b}')),
            outro => panic!("{outro:?}"),
        }
    }

    #[test]
    fn objeto_embutido_e_pulado() {
        assert_eq!(run("<w:object><w:t>x</w:t></w:object><w:t>a</w:t>"), vec![texto("a")]);
    }

    #[test]
    fn alternativa_do_word_nao_duplica_a_imagem() {
        let corpo = r#"<mc:AlternateContent><mc:Choice><w:drawing><wp:docPr id="1" name="x" descr="d"/></w:drawing></mc:Choice><mc:Fallback><w:pict/></mc:Fallback></mc:AlternateContent>"#;
        assert_eq!(run(corpo), vec![Trecho::Imagem("d".into())]);
    }

    #[test]
    fn marcas_do_run_vao_para_o_texto() {
        match &run("<w:rPr><w:b/></w:rPr><w:t>x</w:t>")[..] {
            [Trecho::Texto { marcas, .. }] => assert!(marcas.negrito),
            outro => panic!("{outro:?}"),
        }
    }

    #[test]
    fn marcas_depois_do_texto_tambem_valem() {
        let saida = run("<w:t>a</w:t><w:br/><w:t>b</w:t><w:rPr><w:i/></w:rPr>");
        let italicos = saida
            .iter()
            .filter(|t| matches!(t, Trecho::Texto { marcas, .. } if marcas.italico))
            .count();
        assert_eq!(italicos, 2);
    }

    #[test]
    fn marcas_nao_vazam_para_trechos_anteriores() {
        let mut saida = vec![texto("antes")];
        let xml = r#"<w:r xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:rPr><w:b/></w:rPr><w:t>x</w:t></w:r>"#;
        com_contexto("", "", |ctx| {
            let mut l = Leitor::novo(xml.as_bytes(), &Unlimited);
            l.ate("r").unwrap().unwrap();
            ler_run(&mut l, ctx, None, &mut saida).unwrap();
        });
        assert_eq!(saida[0], texto("antes"));
    }

    #[test]
    fn link_e_aplicado() {
        match &run_com_link("<w:t>x</w:t>", Some("https://a.org"))[..] {
            [Trecho::Texto { link, .. }] => assert_eq!(link.as_deref(), Some("https://a.org")),
            outro => panic!("{outro:?}"),
        }
    }

    #[test]
    fn controle_de_terminal_some() {
        match &run("<w:t>a\u{1b}[31mb</w:t>")[..] {
            [Trecho::Texto { texto, .. }] => assert!(!texto.contains('\u{1b}'), "{texto:?}"),
            outro => panic!("{outro:?}"),
        }
    }
}
