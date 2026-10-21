//! Pré-varredura linear do HTML, antes do `html5ever`: estima quantos elementos ficariam abertos ao
//! mesmo tempo. O parser cresce de forma quadrática com o aninhamento; esta conta recusa a entrada cedo.
//! Regra: a estimativa pode passar do valor real (aí o corpo cai no RTF ou no texto), nunca ficar abaixo.
//! Por isso um fechamento só desempilha quando o parser certamente desempilharia (nenhuma barreira no meio),
//! elementos de formatação contam enquanto estiverem na lista de formatação ativa do parser, e cada
//! `<table>` soma os elementos implícitos (tbody, tr, colgroup) que o parser pode criar.

use crate::MAX_HTML_PARSE_DEPTH;
use std::collections::HashMap;

/// Elementos vazios (fora de conteúdo estrangeiro): nunca ficam abertos.
const VAZIAS: &[&str] = &[
    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source", "track", "wbr",
];
/// Texto cru (RAWTEXT/RCDATA), fora de conteúdo estrangeiro. `noscript` fica de fora: é conservador.
const CRUS: &[&str] = &["iframe", "noembed", "noframes", "script", "style", "textarea", "title", "xmp"];
/// Barreiras: um fechamento não desempilha por cima delas (o parser ignora o fechamento). São os elementos
/// "especiais" do HTML, de MathML e de SVG, mais os limites de escopo; incluir a mais só mantém a pilha maior.
const BARREIRAS: &[&str] = &[
    "address", "applet", "area", "article", "aside", "base", "basefont", "bgsound", "blockquote", "body", "br",
    "button", "caption", "center", "col", "colgroup", "dd", "details", "dir", "div", "dl", "dt", "embed",
    "fieldset", "figcaption", "figure", "footer", "form", "frame", "frameset", "h1", "h2", "h3", "h4", "h5", "h6",
    "head", "header", "hgroup", "hr", "html", "iframe", "img", "input", "keygen", "li", "link", "listing", "main",
    "marquee", "menu", "meta", "nav", "noembed", "noframes", "noscript", "object", "ol", "p", "param",
    "plaintext", "pre", "script", "search", "section", "select", "source", "style", "summary", "table", "tbody",
    "td", "template", "textarea", "tfoot", "th", "thead", "title", "tr", "track", "ul", "wbr", "xmp", "math", "mi",
    "mo", "mn", "ms", "mtext", "annotation-xml", "svg", "foreignobject", "desc",
];
/// Elementos de formatação: o parser os reabre na reconstrução, então contam enquanto estão na lista ativa.
const FORMATACAO: &[&str] = &[
    "a", "b", "big", "code", "em", "font", "i", "nobr", "s", "small", "strike", "strong", "tt", "u",
];
/// Raízes de conteúdo estrangeiro (SVG e MathML): dentro delas, void e texto cru não valem.
const ESTRANGEIROS: &[&str] = &["svg", "math"];
/// Tags de HTML que fazem o parser sair do conteúdo SVG/MathML. `font` é conservador: no parser
/// só sai se tiver color/face/size, mas tratá-lo sempre como saída evita depender do tokenizador de atributos.
const SAIDA_ESTRANGEIRO: &[&str] = &[
    "b", "big", "blockquote", "body", "br", "center", "code", "dd", "div", "dl", "dt", "em", "embed", "font",
    "h1", "h2", "h3", "h4", "h5", "h6", "head", "hr", "i", "img", "li", "listing", "menu", "meta", "nobr",
    "ol", "p", "pre", "ruby", "s", "small", "span", "strong", "strike", "sub", "sup", "table", "tt", "u", "ul",
    "var",
];
/// Elementos implícitos que o parser pode criar sob cada `<table>` (tbody, tr, colgroup).
const EXTRA_POR_TABELA: usize = 3;
pub(crate) const MSG_ANINHADO: &str = "HTML aninhado demais";

/// Recusa o HTML cujo aninhamento estimado passa de `MAX_HTML_PARSE_DEPTH` elementos abertos.
pub(crate) fn confere(html: &str) -> Result<(), String> {
    let b = html.as_bytes();
    let mut pilha = Pilha::novo();
    let mut i = 0;
    while let Some(off) = b[i..].iter().position(|&c| c == b'<') {
        let at = i + off;
        i = match b.get(at + 1) {
            Some(b'!') if pilha.estrangeiro() && b[at..].starts_with(b"<![CDATA[") => fim_cdata(b, at),
            Some(b'!') if b[at..].starts_with(b"<!--") => fim_comentario(b, at),
            Some(b'!' | b'?') => ate(b, at + 2, b'>'),
            Some(b'/') => match b.get(at + 2) {
                Some(c) if c.is_ascii_alphabetic() => {
                    let fim_nome = fim_do_nome(b, at + 2);
                    pilha.fecha(&b[at + 2..fim_nome].to_ascii_lowercase());
                    fim_da_tag(b, fim_nome)
                }
                Some(b'>') => at + 3,
                _ => ate(b, at + 2, b'>'),
            },
            Some(c) if c.is_ascii_alphabetic() => {
                let fim_nome = fim_do_nome(b, at + 1);
                let nome = b[at + 1..fim_nome].to_ascii_lowercase();
                let fim = fim_da_tag(b, fim_nome);
                if pilha.estrangeiro() && esta(SAIDA_ESTRANGEIRO, &nome) {
                    pilha.sai_de_conteudo_estrangeiro();
                }
                let estrangeiro = pilha.estrangeiro();
                if !estrangeiro && esta(VAZIAS, &nome) {
                    fim
                } else if !estrangeiro && esta(CRUS, &nome) {
                    let mut fechamento = b"</".to_vec();
                    fechamento.extend_from_slice(&nome);
                    busca(b, fim, &fechamento).map_or(b.len(), |p| fim_da_tag(b, p + fechamento.len()))
                } else {
                    pilha.abre(nome)?;
                    fim
                }
            }
            _ => at + 1,
        };
    }
    Ok(())
}

/// Pilha estimada: elementos não formatação abertos (com barreiras), mais a contagem de formatação
/// ativa e os implícitos de tabelas.
struct Pilha {
    /// Nome e se é barreira, na ordem da pilha.
    nomes: Vec<(Vec<u8>, bool)>,
    /// Posições na pilha de cada nome (a última é a mais alta).
    posicoes: HashMap<Vec<u8>, Vec<usize>>,
    /// Posições das barreiras, na ordem da pilha.
    barreiras: Vec<usize>,
    /// Quantos elementos de formatação estão na lista ativa, por entrada de `FORMATACAO`.
    fmt: Vec<usize>,
}

impl Pilha {
    fn novo() -> Self {
        Pilha { nomes: Vec::new(), posicoes: HashMap::new(), barreiras: Vec::new(), fmt: vec![0; FORMATACAO.len()] }
    }

    /// Profundidade estimada: o que é certo ou pode existir no parser.
    fn total(&self) -> usize {
        let tabelas = self.posicoes.get(&b"table"[..]).map_or(0, Vec::len);
        self.nomes.len() + EXTRA_POR_TABELA * tabelas + self.fmt.iter().sum::<usize>()
    }

    fn estrangeiro(&self) -> bool {
        ESTRANGEIROS.iter().any(|n| self.posicoes.contains_key(n.as_bytes()))
    }

    /// O tree builder retira o elemento SVG/MathML e seus descendentes antes de tratar estas tags como HTML.
    fn sai_de_conteudo_estrangeiro(&mut self) {
        let Some(raiz) = self
            .nomes
            .iter()
            .rposition(|(nome, _)| ESTRANGEIROS.iter().any(|n| n.as_bytes() == nome.as_slice()))
        else {
            return;
        };
        while self.nomes.len() > raiz {
            self.desempilha();
        }
    }

    fn abre(&mut self, nome: Vec<u8>) -> Result<(), String> {
        if let Some(idx) = FORMATACAO.iter().position(|f| f.as_bytes() == nome.as_slice()) {
            self.fmt[idx] += 1;
        } else {
            let pos = self.nomes.len();
            let barreira = esta(BARREIRAS, &nome);
            self.posicoes.entry(nome.clone()).or_default().push(pos);
            if barreira {
                self.barreiras.push(pos);
            }
            self.nomes.push((nome, barreira));
        }
        if self.total() > MAX_HTML_PARSE_DEPTH {
            return Err(MSG_ANINHADO.to_string());
        }
        Ok(())
    }

    /// Fechamento: formatação só sai da lista ativa (o parser a reabre na reconstrução, então não desempilha).
    /// Outro elemento desempilha até o mais alto com esse nome, se nenhuma barreira estiver no caminho;
    /// senão o parser ignora o fechamento e a estimativa fica alta.
    fn fecha(&mut self, nome: &[u8]) {
        if let Some(idx) = FORMATACAO.iter().position(|f| f.as_bytes() == nome) {
            self.fmt[idx] = self.fmt[idx].saturating_sub(1);
            return;
        }
        let Some(alvo) = self.posicoes.get(nome).and_then(|v| v.last().copied()) else { return };
        if self.barreiras.last().is_some_and(|&l| l > alvo) {
            return;
        }
        while self.nomes.len() > alvo {
            self.desempilha();
        }
    }

    fn desempilha(&mut self) {
        let Some((nome, barreira)) = self.nomes.pop() else { return };
        if let Some(v) = self.posicoes.get_mut(&nome) {
            v.pop();
            if v.is_empty() {
                self.posicoes.remove(&nome);
            }
        }
        if barreira {
            self.barreiras.pop();
        }
    }
}

/// Comentário que começa em `at` (`<!--`): termina em `-->` ou `--!>`; sem fim, vai ao fim.
/// `<!-->` e `<!--->` são comentários vazios, como no parser.
fn fim_comentario(b: &[u8], at: usize) -> usize {
    let de = at + 4;
    if b[de..].starts_with(b">") {
        return de + 1;
    }
    if b[de..].starts_with(b"->") {
        return de + 2;
    }
    let mut j = de;
    while j < b.len() {
        if b[j..].starts_with(b"-->") {
            return j + 3;
        }
        if b[j..].starts_with(b"--!>") {
            return j + 4;
        }
        j += 1;
    }
    b.len()
}

/// CDATA em conteúdo SVG/MathML trata `<` e `>` como texto até `]]>`.
fn fim_cdata(b: &[u8], at: usize) -> usize {
    let de = at + b"<![CDATA[".len();
    b[de..].windows(3).position(|w| w == b"]]>").map_or(b.len(), |p| de + p + 3)
}

/// Posição logo depois do primeiro `c` a partir de `de`; sem ele, vai ao fim.
fn ate(b: &[u8], de: usize, c: u8) -> usize {
    b[de..].iter().position(|&x| x == c).map_or(b.len(), |p| de + p + 1)
}

/// Fim do nome de tag: espaço ASCII, `/` ou `>` (ou o fim da entrada).
fn fim_do_nome(b: &[u8], de: usize) -> usize {
    b[de..]
        .iter()
        .position(|c| c.is_ascii_whitespace() || matches!(c, b'/' | b'>'))
        .map_or(b.len(), |p| de + p)
}

/// Posição de `pat` (sem diferenciar caixa) a partir de `de`.
fn busca(b: &[u8], de: usize, pat: &[u8]) -> Option<usize> {
    b[de..].windows(pat.len()).position(|w| w.eq_ignore_ascii_case(pat)).map(|p| p + de)
}

/// Posição logo depois do `>` que fecha a tag, seguindo o tokenizador: valor de atributo entre aspas
/// só começa logo depois de `=` que segue um nome de atributo (`=` no início de nome é só caractere).
/// Valor sem aspas vai até espaço ou `>`. Sem fim, vai ao fim da entrada.
fn fim_da_tag(b: &[u8], de: usize) -> usize {
    let mut j = de;
    let mut nome = false;
    let mut espera_valor = false;
    while j < b.len() {
        let c = b[j];
        if espera_valor {
            if c.is_ascii_whitespace() {
                j += 1;
                continue;
            }
            espera_valor = false;
            nome = false;
            match c {
                b'"' | b'\'' => match b[j + 1..].iter().position(|&x| x == c) {
                    Some(p) => {
                        j = j + 1 + p + 1;
                        continue;
                    }
                    None => return b.len(),
                },
                b'>' => return j + 1,
                _ => {
                    while j < b.len() && !b[j].is_ascii_whitespace() && b[j] != b'>' {
                        j += 1;
                    }
                    continue;
                }
            }
        }
        match c {
            b'>' => return j + 1,
            b'=' if nome => {
                espera_valor = true;
                nome = false;
            }
            b'=' => nome = true,
            b'/' => nome = false,
            c if c.is_ascii_whitespace() => {}
            _ => nome = true,
        }
        j += 1;
    }
    b.len()
}

fn esta(lista: &[&str], nome: &[u8]) -> bool {
    lista.iter().any(|v| v.as_bytes() == nome)
}

#[cfg(test)]
mod tests {
    use super::confere;
    use scraper::{ElementRef, Html};

    /// Profundidade que o html5ever monta de fato: ancestrais do último elemento (o mais fundo numa cadeia).
    fn profundidade_real(h: &str) -> usize {
        let doc = Html::parse_document(h);
        doc.root_element()
            .descendants()
            .filter_map(ElementRef::wrap)
            .last()
            .map_or(0, |el| el.ancestors().count())
    }

    #[test]
    fn fechamento_sem_abertura_nao_diminui_a_conta() {
        let h = "<center></foo>".repeat(6000);
        assert!(profundidade_real(&h) >= 5000);
        assert!(confere(&h).is_err());
    }

    #[test]
    fn fechamento_estranho_nao_reduz_profundidade() {
        let h = "<div></span>".repeat(6000);
        assert!(profundidade_real(&h) >= 5000);
        assert!(confere(&h).is_err());
    }

    #[test]
    fn nome_de_tag_com_simbolos_conta() {
        for h in ["<a:b>".repeat(6000), "<d\u{0}iv>".repeat(6000)] {
            assert!(profundidade_real(&h) >= 5000);
            assert!(confere(&h).is_err());
        }
    }

    #[test]
    fn comentario_com_bang_termina() {
        let h = "<!-- x --!>".to_string() + &"<div>".repeat(6000);
        assert!(profundidade_real(&h) >= 5000);
        assert!(confere(&h).is_err());
    }

    #[test]
    fn comentario_sem_fim_vai_ao_fim() {
        assert!(confere(&("<!-- x".to_string() + &"<div>".repeat(6000))).is_ok());
    }

    #[test]
    fn style_dentro_de_svg_nao_e_texto_cru() {
        let h = "<svg><style>".to_string() + &"<g>".repeat(6000);
        assert!(profundidade_real(&h) >= 5000);
        assert!(confere(&h).is_err());
    }

    #[test]
    fn cdata_em_svg_nao_esconde_aninhamento_depois_de_fechamento_falso() {
        let h = "<svg><![CDATA[></svg>]]><style>".to_string() + &"<g>".repeat(5100);
        assert!(profundidade_real(&h) >= 5000);
        assert!(confere(&h).is_err());
    }

    #[test]
    fn cdata_depois_de_escape_de_conteudo_estrangeiro_nao_esconde_tags() {
        for h in [
            "<svg><p><![CDATA[>".to_string() + &"<div>".repeat(5100),
            "<math><p><![CDATA[>".to_string() + &"<div>".repeat(5100) + "]]>",
        ] {
            assert!(profundidade_real(&h) >= 5000);
            assert!(confere(&h).is_err());
        }
    }

    #[test]
    fn igual_no_inicio_do_atributo_nao_abre_valor() {
        let h = "<center =\">".repeat(6000);
        assert!(profundidade_real(&h) >= 5000);
        assert!(confere(&h).is_err());
    }

    #[test]
    fn formatacao_aberta_conta_e_fechada_nao() {
        assert!(confere(&"<b>".repeat(6000)).is_err());
        assert!(confere(&"<b>x</b>".repeat(6000)).is_ok());
    }

    #[test]
    fn tabelas_aninhadas_contam_os_implicitos() {
        let h = "<table><td>".repeat(1500);
        assert!(profundidade_real(&h) >= 5000);
        assert!(confere(&h).is_err());
    }

    #[test]
    fn vazias_e_texto_cru_nao_contam() {
        let h = "<br><img src=x>".repeat(6000) + &"<script>a<b<c</script>".repeat(6000);
        assert!(confere(&h).is_ok());
    }

    #[test]
    fn valor_entre_aspas_com_maior_que_nao_fecha_tag() {
        assert!(confere(r#"<a title="<div>"><p>x</p>"#).is_ok());
    }
}
