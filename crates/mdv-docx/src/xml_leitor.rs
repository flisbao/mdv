//! Leitor de XML: envolve o `quick-xml` e entrega eventos simples com nome local.
//! É o único arquivo da crate que conhece o `quick-xml`.

use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;

use crate::{Budget, MAX_PROFUNDIDADE_XML};

/// Teto de atributos por elemento; o Word usa poucas dezenas (o raiz, com `xmlns:*`, é o maior).
const MAX_ATRIBUTOS: usize = 256;

/// Um evento do documento: abre elemento, fecha elemento ou texto já sem entidades.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Evento {
    Abre(Tag),
    Fecha(String),
    Texto(String),
}

/// Elemento aberto: nome local e atributos (nome local, valor).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Tag {
    pub nome: String,
    pub atributos: Vec<(String, String)>,
}

impl Tag {
    /// Valor do atributo pelo nome local (`"val"` acha `w:val`).
    pub(crate) fn attr(&self, nome: &str) -> Option<&str> {
        self.atributos.iter().find(|(n, _)| n == nome).map(|(_, v)| v.as_str())
    }

    /// Atalho de `attr("val")`.
    pub(crate) fn val(&self) -> Option<&str> {
        self.attr("val")
    }

    /// Propriedade booleana do Word: sem `val` vale ligada; `0`, `false` e `off` desligam.
    pub(crate) fn ligado(&self) -> bool {
        !matches!(self.val(), Some("0" | "false" | "off"))
    }
}

/// Leitor de eventos sobre um XML em memória, com teto de profundidade e de orçamento.
pub(crate) struct Leitor<'a> {
    xml: Reader<&'a [u8]>,
    budget: &'a dyn Budget,
    profundidade: usize,
    /// `Fecha` de um elemento vazio, devolvido na chamada seguinte.
    pendente: Option<Evento>,
}

impl<'a> Leitor<'a> {
    /// Cria o leitor; texto não é aparado e fim sem par de abertura é erro.
    pub(crate) fn novo(xml: &'a [u8], budget: &'a dyn Budget) -> Self {
        let mut xml = Reader::from_reader(xml);
        let config = xml.config_mut();
        config.trim_text(false);
        config.check_end_names = true;
        Leitor { xml, budget, profundidade: 0, pendente: None }
    }

    /// Próximo evento, ou `None` no fim do documento.
    pub(crate) fn proximo(&mut self) -> Result<Option<Evento>, String> {
        self.budget.check()?;
        if let Some(fecha) = self.pendente.take() {
            self.profundidade -= 1;
            return Ok(Some(fecha));
        }
        loop {
            let evento = self.xml.read_event().map_err(|e| self.erro(e))?;
            match evento {
                Event::Start(s) => return self.abrir(&s, false).map(Some),
                Event::Empty(s) => return self.abrir(&s, true).map(Some),
                Event::End(e) => {
                    let nome = self.nome(e.local_name().as_ref())?;
                    self.profundidade = self.profundidade.saturating_sub(1);
                    return Ok(Some(Evento::Fecha(nome)));
                }
                Event::Text(t) => {
                    let texto = t.unescape().map_err(|e| self.erro(e))?;
                    // Espaço solto fora do elemento raiz não é conteúdo.
                    if self.profundidade > 0 && !texto.is_empty() {
                        return Ok(Some(Evento::Texto(texto.into_owned())));
                    }
                }
                Event::CData(c) => {
                    let texto = c.decode().map_err(|e| self.erro(e))?;
                    if self.profundidade > 0 && !texto.is_empty() {
                        return Ok(Some(Evento::Texto(texto.into_owned())));
                    }
                }
                // Recusar o DTD evita a expansão de entidades (bomba "billion laughs").
                Event::DocType(_) => return Err(self.erro("DTD não é permitido")),
                Event::Eof if self.profundidade > 0 => {
                    return Err(self.erro("fim inesperado do documento"));
                }
                Event::Eof => return Ok(None),
                Event::Comment(_) | Event::Decl(_) | Event::PI(_) => {}
            }
        }
    }

    /// Consome até o `Fecha` do elemento recém-aberto, descartando tudo o que ele contém.
    pub(crate) fn pular(&mut self) -> Result<(), String> {
        // Depois do `Abre` a profundidade já o inclui; o elemento fecha ao voltar um nível.
        // Um elemento vazio conta igual, pois o `Fecha` pendente também baixa a profundidade.
        let alvo = self.profundidade.saturating_sub(1);
        while self.profundidade > alvo {
            if self.proximo()?.is_none() {
                return Err(self.erro("fim inesperado do documento"));
            }
        }
        Ok(())
    }

    /// Descarta eventos até o próximo `Abre` com esse nome local; `None` se o documento acabar.
    pub(crate) fn ate(&mut self, nome: &str) -> Result<Option<Tag>, String> {
        // Os `Fecha` e o texto no caminho são descartados: quem chama só quer o elemento.
        while let Some(evento) = self.proximo()? {
            if let Evento::Abre(tag) = evento {
                if tag.nome == nome {
                    return Ok(Some(tag));
                }
            }
        }
        Ok(None)
    }

    /// Converte um início de elemento em `Abre`; elementos vazios deixam o `Fecha` pendente.
    fn abrir(&mut self, s: &BytesStart, vazio: bool) -> Result<Evento, String> {
        // Conta o elemento vazio também: o `Fecha` pendente devolve a profundidade ao valor anterior.
        if self.profundidade + 1 > MAX_PROFUNDIDADE_XML {
            return Err(self.erro(format!("profundidade acima de {MAX_PROFUNDIDADE_XML}")));
        }
        let nome = self.nome(s.local_name().as_ref())?;
        let mut atributos = Vec::new();
        for atributo in s.attributes() {
            // O quick-xml compara cada atributo novo com todos os anteriores (custo quadrático):
            // o teto vale durante a iteração, antes de coletar o resto.
            if atributos.len() >= MAX_ATRIBUTOS {
                return Err(self.erro("atributos demais"));
            }
            self.budget.check()?;
            let atributo = atributo.map_err(|e| self.erro(e))?;
            let chave = self.nome(atributo.key.local_name().as_ref())?;
            let valor = atributo.unescape_value().map_err(|e| self.erro(e))?;
            atributos.push((chave, valor.into_owned()));
        }
        self.profundidade += 1;
        if vazio {
            self.pendente = Some(Evento::Fecha(nome.clone()));
        }
        Ok(Evento::Abre(Tag { nome, atributos }))
    }

    /// Nome em bytes para `String`; UTF-8 inválido é XML inválido.
    fn nome(&self, bytes: &[u8]) -> Result<String, String> {
        std::str::from_utf8(bytes)
            .map(str::to_owned)
            .map_err(|_| self.erro("nome com UTF-8 inválido"))
    }

    /// Mensagem de erro padrão, com a posição em bytes.
    fn erro(&self, detalhe: impl std::fmt::Display) -> String {
        format!("XML inválido (byte {}): {detalhe}", self.xml.error_position())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Unlimited;

    fn abre(nome: &str) -> Evento {
        Evento::Abre(Tag { nome: nome.into(), atributos: vec![] })
    }
    fn fecha(nome: &str) -> Evento {
        Evento::Fecha(nome.into())
    }
    fn texto(t: &str) -> Evento {
        Evento::Texto(t.into())
    }

    fn todos(xml: &str) -> Result<Vec<Evento>, String> {
        let mut leitor = Leitor::novo(xml.as_bytes(), &Unlimited);
        let mut v = Vec::new();
        while let Some(e) = leitor.proximo()? {
            v.push(e);
        }
        Ok(v)
    }

    fn primeira_tag(xml: &str) -> Tag {
        match Leitor::novo(xml.as_bytes(), &Unlimited).proximo() {
            Ok(Some(Evento::Abre(t))) => t,
            outro => panic!("esperava Abre, veio {outro:?}"),
        }
    }

    #[test]
    fn eventos_basicos() {
        let v = todos("<w:p><w:r><w:t>oi</w:t></w:r></w:p>").unwrap();
        let esperado = vec![
            abre("p"), abre("r"), abre("t"), texto("oi"), fecha("t"), fecha("r"), fecha("p"),
        ];
        assert_eq!(v, esperado);
    }

    #[test]
    fn elemento_vazio_gera_abre_e_fecha() {
        assert_eq!(todos("<w:b/>").unwrap(), vec![abre("b"), fecha("b")]);
    }

    #[test]
    fn atributos_por_nome_local() {
        let t = primeira_tag(r#"<w:pStyle w:val="Heading1"/>"#);
        assert_eq!(t.val(), Some("Heading1"));
        let t = primeira_tag(r#"<w:hyperlink r:id="rId5">"#);
        assert_eq!(t.attr("id"), Some("rId5"));
        assert_eq!(t.attr("nada"), None);
    }

    #[test]
    fn entidades_viram_caracteres() {
        let v = todos("<w:t>a &amp; b &#233; &#xE9; &lt;&gt;&quot;&apos;</w:t>").unwrap();
        assert_eq!(v[1], texto("a & b é é <>\"'"));
        let t = primeira_tag(r#"<a x="1 &lt; 2"/>"#);
        assert_eq!(t.attr("x"), Some("1 < 2"));
    }

    #[test]
    fn entidade_desconhecida_da_erro() {
        let e = todos("<a>&nbsp;</a>").unwrap_err();
        assert!(e.contains("XML inválido"), "{e}");
        let e = todos(r#"<a x="&nbsp;"/>"#).unwrap_err();
        assert!(e.contains("XML inválido"), "{e}");
    }

    #[test]
    fn cdata_vira_texto() {
        let v = todos("<a><![CDATA[x < y & z]]></a>").unwrap();
        assert_eq!(v[1], texto("x < y & z"));
    }

    #[test]
    fn comentarios_e_declaracao_sao_pulados() {
        let v = todos("<?xml version=\"1.0\"?>\n<!-- c --><a><?pi x?>t</a>\n").unwrap();
        assert_eq!(v, vec![abre("a"), texto("t"), fecha("a")]);
    }

    #[test]
    fn espacos_dentro_do_texto_sao_preservados() {
        let v = todos("<w:t xml:space=\"preserve\"> a  </w:t>").unwrap();
        assert_eq!(v[1], texto(" a  "));
    }

    #[test]
    fn doctype_e_recusado() {
        let e = todos(r#"<!DOCTYPE x [<!ENTITY a "b">]><w:p>&a;</w:p>"#).unwrap_err();
        assert!(e.contains("DTD"), "{e}");
    }

    fn aninhado(n: usize) -> String {
        format!("{}{}", "<a>".repeat(n), "</a>".repeat(n))
    }

    #[test]
    fn profundidade_acima_do_teto() {
        let e = todos(&aninhado(MAX_PROFUNDIDADE_XML + 1)).unwrap_err();
        assert!(e.contains("profundidade"), "{e}");
        assert!(todos(&aninhado(MAX_PROFUNDIDADE_XML)).is_ok());
    }

    #[test]
    fn pular_consome_ate_o_fecha_do_elemento() {
        let mut l = Leitor::novo(b"<a><b><c/></b><d/></a>", &Unlimited);
        assert!(l.ate("b").unwrap().is_some());
        l.pular().unwrap();
        assert_eq!(l.proximo().unwrap(), Some(abre("d")));
    }

    #[test]
    fn pular_elemento_vazio() {
        let mut l = Leitor::novo(b"<a><b/><d/></a>", &Unlimited);
        assert!(l.ate("b").unwrap().is_some());
        l.pular().unwrap();
        assert_eq!(l.proximo().unwrap(), Some(abre("d")));
    }

    #[test]
    fn ate_devolve_none_no_fim() {
        let mut l = Leitor::novo(b"<a><b/></a>", &Unlimited);
        assert_eq!(l.ate("z").unwrap(), None);
    }

    #[test]
    fn xml_mal_formado_da_erro() {
        let e = todos("<a><b></a>").unwrap_err();
        assert!(e.contains("XML inválido"), "{e}");
    }

    #[test]
    fn documento_truncado_da_erro() {
        let e = todos("<a><b>").unwrap_err();
        assert!(e.contains("XML inválido"), "{e}");
    }

    #[test]
    fn ligado_entende_val_falso() {
        assert!(primeira_tag("<w:b/>").ligado());
        for v in ["0", "false", "off"] {
            assert!(!primeira_tag(&format!(r#"<w:b w:val="{v}"/>"#)).ligado(), "{v}");
        }
        for v in ["1", "true", "on"] {
            assert!(primeira_tag(&format!(r#"<w:b w:val="{v}"/>"#)).ligado(), "{v}");
        }
    }

    fn com_atributos(n: usize) -> String {
        let attrs: String = (0..n).map(|i| format!(" a{i}=\"\"")).collect();
        format!("<a{attrs}/>")
    }

    #[test]
    fn atributos_acima_do_teto_dao_erro() {
        let e = todos(&com_atributos(MAX_ATRIBUTOS + 1)).unwrap_err();
        assert!(e.contains("XML inválido") && e.contains("atributos demais"), "{e}");
    }

    #[test]
    fn atributos_no_teto_passam() {
        let t = primeira_tag(&com_atributos(MAX_ATRIBUTOS));
        assert_eq!(t.atributos.len(), MAX_ATRIBUTOS);
    }

    struct Esgotado;
    impl Budget for Esgotado {
        fn check(&self) -> Result<(), String> {
            Err("sem orçamento".into())
        }
    }

    #[test]
    fn orcamento_esgotado_para_a_leitura() {
        let mut l = Leitor::novo(b"<a/>", &Esgotado);
        assert!(l.proximo().is_err());
    }
}
