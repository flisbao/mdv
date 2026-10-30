//! Conteúdo de um run (`w:r`): texto, quebras, referências de nota e imagens.
//! Só lê o run; quem agrupa os runs em parágrafos e decide o link fica no módulo de parágrafo.

use crate::contexto::Contexto;
use crate::estilo_run::ler_rpr;
use crate::modelo::Trecho;
use crate::xml_leitor::Leitor;

use crate::modelo::{chave_nota, Marcas};
use crate::xml_leitor::Evento;
use mdv_text::sanitize;

/// Lê um `w:r` (chamada logo depois do `Abre("r")`) e consome até o `Fecha` dele.
pub(crate) fn ler_run(
    l: &mut Leitor,
    ctx: &Contexto,
    link: Option<&str>,
    saida: &mut Vec<Trecho>,
) -> Result<(), String> {
    // O `w:rPr` pode vir depois do texto no XML: os trechos saem com marcas padrão e recebem as
    // do run no fim, a partir deste índice.
    let inicio = saida.len();
    let mut marcas = Marcas::default();
    let mut pendente = String::new();
    loop {
        match l.proximo()? {
            None => return Err("fim inesperado do documento".into()),
            Some(Evento::Fecha(n)) if n == "r" => break,
            Some(Evento::Abre(tag)) => match tag.nome.as_str() {
                "rPr" => marcas = ler_rpr(l, ctx)?,
                // Só `w:t` é texto do documento; `delText` e `instrText` (abaixo) são pulados.
                "t" => ler_t(l, &mut pendente)?,
                "tab" => pendente.push(' '),
                "noBreakHyphen" => pendente.push('-'),
                "br" => {
                    // Quebra de página ou de coluna não é quebra de linha no Markdown.
                    if matches!(tag.attr("type"), None | Some("textWrapping")) {
                        esvaziar(&mut pendente, link, saida);
                        saida.push(Trecho::Quebra);
                    }
                }
                "footnoteReference" | "endnoteReference" => {
                    esvaziar(&mut pendente, link, saida);
                    let fim = tag.nome == "endnoteReference";
                    saida.push(Trecho::NotaRef(chave_nota(fim, tag.attr("id").unwrap_or(""))));
                }
                // Desenho é consumido inteiro só para ler o `descr`; o que há dentro (caixas de
                // texto, formas) é descartado, pois a conversão não trata caixas de texto.
                "drawing" | "pict" => {
                    esvaziar(&mut pendente, link, saida);
                    let descr = ler_desenho(l)?;
                    saida.push(Trecho::Imagem(sanitize(&descr).into_owned()));
                }
                // O Word repete o desenho em `Fallback` para versões antigas; só vale o `Choice`.
                "object" | "delText" | "instrText" | "Fallback" => l.pular()?,
                _ => {}
            },
            Some(_) => {}
        }
    }
    esvaziar(&mut pendente, link, saida);
    for t in &mut saida[inicio..] {
        if let Trecho::Texto { marcas: m, .. } = t {
            *m = marcas;
        }
    }
    Ok(())
}

/// Lê o texto de um `w:t` (já aberto) até o `Fecha` dele, juntando em `pendente`.
fn ler_t(l: &mut Leitor, pendente: &mut String) -> Result<(), String> {
    loop {
        match l.proximo()? {
            None => return Err("fim inesperado do documento".into()),
            Some(Evento::Fecha(n)) if n == "t" => return Ok(()),
            Some(Evento::Texto(t)) => pendente.push_str(&t),
            Some(_) => {}
        }
    }
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

/// Passa o texto acumulado (já sem controles de terminal) para a saída; vazio não gera trecho.
fn esvaziar(pendente: &mut String, link: Option<&str>, saida: &mut Vec<Trecho>) {
    if pendente.is_empty() {
        return;
    }
    let texto = sanitize(pendente).into_owned();
    pendente.clear();
    saida.push(Trecho::Texto { texto, marcas: Marcas::default(), link: link.map(str::to_string) });
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
            ler_run(&mut l, ctx, link, &mut saida).unwrap();
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
