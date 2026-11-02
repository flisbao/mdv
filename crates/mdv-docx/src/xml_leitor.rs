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
    /// Nomes dos elementos abertos, do raiz ao mais fundo; o tamanho é a profundidade.
    abertos: Vec<String>,
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
        Leitor { xml, budget, abertos: Vec::new(), pendente: None }
    }

    /// Próximo evento, ou `None` no fim do documento.
    pub(crate) fn proximo(&mut self) -> Result<Option<Evento>, String> {
        self.budget.check()?;
        if let Some(fecha) = self.pendente.take() {
            self.abertos.pop();
            return Ok(Some(fecha));
        }
        loop {
            let evento = self.xml.read_event().map_err(|e| self.erro(e))?;
            match evento {
                Event::Start(s) => return self.abrir(&s, false).map(Some),
                Event::Empty(s) => return self.abrir(&s, true).map(Some),
                Event::End(e) => {
                    let nome = self.nome(e.local_name().as_ref())?;
                    self.abertos.pop();
                    return Ok(Some(Evento::Fecha(nome)));
                }
                Event::Text(t) => {
                    let texto = t.unescape().map_err(|e| self.erro(e))?;
                    // Espaço solto fora do elemento raiz não é conteúdo.
                    if !self.abertos.is_empty() && !texto.is_empty() {
                        return Ok(Some(Evento::Texto(texto.into_owned())));
                    }
                }
                Event::CData(c) => {
                    let texto = c.decode().map_err(|e| self.erro(e))?;
                    if !self.abertos.is_empty() && !texto.is_empty() {
                        return Ok(Some(Evento::Texto(texto.into_owned())));
                    }
                }
                // Recusar o DTD evita a expansão de entidades (bomba "billion laughs").
                Event::DocType(_) => return Err(self.erro("DTD não é permitido")),
                Event::Eof if !self.abertos.is_empty() => {
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
        let alvo = self.abertos.len().saturating_sub(1);
        while self.abertos.len() > alvo {
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

    /// Próximo evento dentro do elemento `nome`, ou `None` no `Fecha` dele.
    ///
    /// O elemento é o aberto mais recente com esse nome; o `Fecha` que o encerra é o que devolve a
    /// profundidade ao nível dele, então o `Fecha` de um descendente de mesmo nome não o encerra
    /// (os `Fecha` dos descendentes são descartados). Quem tratar um filho de mesmo nome deve
    /// consumi-lo por inteiro (`pular` ou chamando `dentro` de novo). O fim do documento antes do
    /// `Fecha` é o erro `XML inválido` de sempre.
    pub(crate) fn dentro(&mut self, nome: &str) -> Result<Option<Evento>, String> {
        let nivel = match self.abertos.iter().rposition(|n| n == nome) {
            Some(i) => i,
            None => return Err(self.erro(format!("`{nome}` não está aberto"))),
        };
        loop {
            match self.proximo()? {
                None => return Err(self.erro("fim inesperado do documento")),
                Some(Evento::Fecha(_)) if self.abertos.len() == nivel => return Ok(None),
                Some(Evento::Fecha(_)) => {}
                Some(evento) => return Ok(Some(evento)),
            }
        }
    }

    /// Converte um início de elemento em `Abre`; elementos vazios deixam o `Fecha` pendente.
    fn abrir(&mut self, s: &BytesStart, vazio: bool) -> Result<Evento, String> {
        // Conta o elemento vazio também: o `Fecha` pendente devolve a profundidade ao valor anterior.
        if self.abertos.len() + 1 > MAX_PROFUNDIDADE_XML {
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
        self.abertos.push(nome.clone());
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
        mdv_text::sanitize(&format!("XML inválido (byte {}): {detalhe}", self.xml.error_position())).into_owned()
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
    fn erro_de_xml_com_controle_nao_vai_cru_para_a_mensagem() {
        // O nome da tag de fechamento (vindo do arquivo) aparece na mensagem do quick-xml.
        let erro = todos("<a></b\u{1b}c>").unwrap_err();
        assert!(erro.contains("XML inválido"), "{erro}");
        assert!(!erro.chars().any(char::is_control), "{erro:?}");
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
    fn dentro_entrega_filhos_e_texto_ate_o_fecha_do_elemento() {
        let mut l = Leitor::novo(b"<a><b>x</b><c/></a><d/>", &Unlimited);
        l.ate("a").unwrap().unwrap();
        assert_eq!(l.dentro("a").unwrap(), Some(abre("b")));
        assert_eq!(l.dentro("a").unwrap(), Some(texto("x")));
        assert_eq!(l.dentro("a").unwrap(), Some(abre("c")));
        assert_eq!(l.dentro("a").unwrap(), None);
        assert_eq!(l.proximo().unwrap(), Some(abre("d")), "o Fecha do elemento é consumido");
    }

    #[test]
    fn dentro_de_elemento_vazio_termina_de_imediato() {
        let mut l = Leitor::novo(b"<a/><d/>", &Unlimited);
        l.ate("a").unwrap().unwrap();
        assert_eq!(l.dentro("a").unwrap(), None);
        assert_eq!(l.proximo().unwrap(), Some(abre("d")));
    }

    #[test]
    fn dentro_ignora_o_fecha_dos_descendentes() {
        // Os `Fecha` de `b` e `c` não aparecem nem encerram o laço.
        let mut l = Leitor::novo(b"<a><b><c/></b></a>", &Unlimited);
        l.ate("a").unwrap().unwrap();
        assert_eq!(l.dentro("a").unwrap(), Some(abre("b")));
        assert_eq!(l.dentro("a").unwrap(), Some(abre("c")));
        assert_eq!(l.dentro("a").unwrap(), None);
    }

    #[test]
    fn dentro_compara_o_nivel_e_nao_so_o_nome() {
        // O `a` interno tem o mesmo nome do externo: o `Fecha` dele não encerra o externo.
        let mut l = Leitor::novo(b"<a><a><b/></a><c/></a><d/>", &Unlimited);
        l.ate("a").unwrap().unwrap();
        assert_eq!(l.dentro("a").unwrap(), Some(abre("a")));
        // Quem trata o filho de mesmo nome o consome por inteiro (aqui, recursivamente).
        assert_eq!(l.dentro("a").unwrap(), Some(abre("b")));
        assert_eq!(l.dentro("a").unwrap(), None, "fecha o `a` interno");
        assert_eq!(l.dentro("a").unwrap(), Some(abre("c")));
        assert_eq!(l.dentro("a").unwrap(), None, "fecha o `a` externo");
        assert_eq!(l.proximo().unwrap(), Some(abre("d")));
    }

    #[test]
    fn dentro_com_filho_pulado_continua_no_mesmo_nivel() {
        let mut l = Leitor::novo(b"<a><a><b/></a><c/></a><d/>", &Unlimited);
        l.ate("a").unwrap().unwrap();
        assert_eq!(l.dentro("a").unwrap(), Some(abre("a")));
        l.pular().unwrap();
        assert_eq!(l.dentro("a").unwrap(), Some(abre("c")));
        assert_eq!(l.dentro("a").unwrap(), None);
        assert_eq!(l.proximo().unwrap(), Some(abre("d")));
    }

    #[test]
    fn dentro_no_fim_do_documento_da_erro_com_o_prefixo_padrao() {
        let mut l = Leitor::novo(b"<a><b/>", &Unlimited);
        l.ate("a").unwrap().unwrap();
        assert_eq!(l.dentro("a").unwrap(), Some(abre("b")));
        assert!(l.dentro("a").unwrap_err().contains("XML inválido"));
    }

    #[test]
    fn dentro_de_elemento_que_nao_esta_aberto_da_erro() {
        let mut l = Leitor::novo(b"<a><b/></a>", &Unlimited);
        l.ate("b").unwrap().unwrap();
        let e = l.dentro("z").unwrap_err();
        assert!(e.contains("XML inválido") && e.contains("não está aberto"), "{e}");
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
