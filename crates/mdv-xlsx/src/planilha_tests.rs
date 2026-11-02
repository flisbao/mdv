use crate::estilos::Estilos;
use crate::limites::{Limites, Orcamento};
use crate::modelo::{Aba, Celula, Corte};
use crate::planilha::{ler, ContextoPasta, Linha};
use crate::Unlimited;

/// Dono dos dados que o `ContextoPasta` só empresta.
struct PastaTeste {
    textos: Vec<String>,
    estilos: Estilos,
    limites: Limites,
}

impl PastaTeste {
    fn ctx(&self) -> ContextoPasta<'_> {
        ContextoPasta { textos: &self.textos, estilos: &self.estilos, data_1904: false, limites: &self.limites }
    }

    fn com_limites(limites: Limites) -> Self {
        PastaTeste { limites, ..ctx_padrao() }
    }

    fn com_textos(textos: &[&str]) -> Self {
        PastaTeste { textos: textos.iter().map(|t| t.to_string()).collect(), ..ctx_padrao() }
    }

    fn com_estilos(estilos: &str) -> Self {
        let xml = format!("<styleSheet>{estilos}</styleSheet>");
        PastaTeste { estilos: Estilos::carregar(xml.as_bytes(), &Unlimited).unwrap(), ..ctx_padrao() }
    }
}

/// Pasta sem textos nem estilos, com os tetos de produção.
fn ctx_padrao() -> PastaTeste {
    PastaTeste { textos: Vec::new(), estilos: Estilos::vazio(), limites: Limites::default() }
}

/// Lê a aba formada pelas linhas de `sheetData`, gastando do `orcamento` dado.
fn ler_aba(linhas: &str, pasta: &PastaTeste, orcamento: &mut Orcamento) -> Aba {
    let xml = format!("<worksheet><sheetData>{linhas}</sheetData></worksheet>");
    ler(xml.as_bytes(), &pasta.ctx(), orcamento, &Unlimited).unwrap()
}

/// Lê sem teto de bytes, com a pasta dada.
fn aba(linhas: &str, pasta: &PastaTeste) -> Aba {
    ler_aba(linhas, pasta, &mut Orcamento::novo(usize::MAX))
}

fn textos(a: &Aba) -> Vec<Vec<&str>> {
    a.linhas.iter().map(|l| l.iter().map(|c| c.texto.as_str()).collect()).collect()
}

#[test]
fn formula_mostra_o_valor_em_cache_e_nunca_a_formula() {
    // A1 = concatenação com t="str" (cache "ab"); A2 = 1+2 (cache 3)
    let a = aba(r#"<row r="1"><c r="A1" t="str"><f>"a"&amp;"b"</f><v>ab</v></c></row>
                   <row r="2"><c r="A2"><f>1+2</f><v>3</v></c></row>"#, &ctx_padrao());
    assert_eq!(textos(&a), [["ab"], ["3"]]);
}
#[test]
fn formula_sem_valor_em_cache_vira_celula_vazia() {
    let a = aba(r#"<row r="1"><c r="A1"><v>1</v></c><c r="B1"><f>A1+1</f></c></row>"#, &ctx_padrao());
    assert_eq!(textos(&a), [["1", ""]]);
}
#[test]
fn tipos_de_celula() {
    // s -> textos[1]; b -> VERDADEIRO/FALSO; e -> texto do erro; inlineStr; d -> ISO como está
    let xml = r#"<row r="1"><c r="A1" t="s"><v>1</v></c><c r="B1" t="b"><v>1</v></c><c r="C1" t="b"><v>0</v></c>
        <c r="D1" t="e"><v>#DIV/0!</v></c><c r="E1" t="inlineStr"><is><t>ola</t></is></c><c r="F1" t="d"><v>2024-01-02</v></c></row>"#;
    assert_eq!(textos(&aba(xml, &PastaTeste::com_textos(&["x", "y"]))), [["y", "VERDADEIRO", "FALSO", "#DIV/0!", "ola", "2024-01-02"]]);
}
#[test]
fn numero_usa_o_formato_do_estilo_e_marca_a_celula_como_numerica() {
    let est = r#"<cellXfs count="2"><xf numFmtId="0"/><xf numFmtId="14"/></cellXfs>"#;
    let a = aba(r#"<row r="1"><c r="A1" s="1"><v>45000</v></c><c r="B1"><v>0.30000000000000004</v></c></row>"#, &PastaTeste::com_estilos(est));
    assert_eq!(textos(&a), [["2023-03-15", "0.3"]]);
    assert!(a.linhas[0][0].numerica && a.linhas[0][1].numerica);
}
#[test]
fn descarta_linhas_vazias_apara_colunas_e_preenche_buracos() {
    let a = aba(r#"<row r="2"><c r="C2"><v>1</v></c><c r="E2"><v>2</v></c></row><row r="5"/><row r="9"><c r="C9"><v>3</v></c></row>"#, &ctx_padrao());
    assert_eq!(textos(&a), [["1", "", "2"], ["3", "", ""]]);
    assert_eq!(a.linhas_total, 2);
}
#[test]
fn coordenada_gigante_nao_aloca_nem_vira_coluna() {
    let p = PastaTeste::com_limites(Limites { max_colunas: 4, ..Limites::default() });
    let a = aba(r#"<row r="1048576"><c r="A1048576"><v>1</v></c><c r="XFD1048576"><v>9</v></c></row>"#, &p);
    assert_eq!(textos(&a), [["1"]]);
    assert_eq!(a.colunas_omitidas, Some(4));
}
#[test]
fn teto_de_linhas_corta_e_conta_o_total() {
    let p = PastaTeste::com_limites(Limites { max_linhas: 2, ..Limites::default() });
    let a = aba(r#"<row r="1"><c r="A1"><v>1</v></c></row><row r="2"><c r="A2"><v>2</v></c></row><row r="3"><c r="A3"><v>3</v></c></row>"#, &p);
    assert_eq!((a.linhas.len(), a.linhas_total), (2, 3));
}
#[test]
fn teto_de_celulas_da_grade_corta_linhas() {
    let linhas: String = (1..=5)
        .map(|r| format!(r#"<row r="{r}"><c r="A{r}"><v>1</v></c><c r="B{r}"><v>2</v></c><c r="C{r}"><v>3</v></c></row>"#))
        .collect();
    let a = aba(&linhas, &PastaTeste::com_limites(Limites { max_celulas: 6, ..Limites::default() }));
    assert_eq!(a.linhas.len(), 2);
    assert_eq!(a.linhas_total, 5);
    assert_eq!(a.corte, Some(Corte::Celulas { teto: 6 }));
    let inteira = aba(&linhas, &PastaTeste::com_limites(Limites { max_celulas: 15, ..Limites::default() }));
    assert_eq!(inteira.linhas.len(), 5);
    assert!(inteira.corte.is_none());
}

#[test]
fn teto_de_celulas_guardadas_para_de_guardar_linhas_em_ordem() {
    // 6 células guardadas no total: duas linhas de 3 entram, a terceira corta; as seguintes não entram.
    let linhas: String = (1..=5)
        .map(|r| format!(r#"<row r="{r}"><c r="A{r}"><v>{r}</v></c><c r="B{r}"><v>2</v></c><c r="C{r}"><v>3</v></c></row>"#))
        .collect();
    let a = aba(&linhas, &PastaTeste::com_limites(Limites { max_celulas: 6, ..Limites::default() }));
    assert_eq!(textos(&a), [["1", "2", "3"], ["2", "2", "3"]]);
    assert_eq!((a.linhas_total, a.corte), (5, Some(Corte::Celulas { teto: 6 })));
}

#[test]
fn teto_de_bytes_para_a_leitura() {
    let mut orc = Orcamento::novo(5);
    let a = ler_aba(r#"<row r="1"><c r="A1" t="inlineStr"><is><t>abcdef</t></is></c></row><row r="2"><c r="A2"><v>1</v></c></row>"#, &ctx_padrao(), &mut orc);
    assert_eq!(a.corte, Some(Corte::Bytes));
    // `abcdef` custa 6 e o orçamento é 5: nada entra e nada é gasto.
    assert!(a.linhas.is_empty());
    assert_eq!(orc.restante(), 5);
}
#[test]
fn orcamento_recusa_o_que_nao_cabe_sem_mudar_nada() {
    let mut orc = Orcamento::novo(5);
    assert!(orc.gastar(3));
    assert!(!orc.gastar(3));
    assert_eq!(orc.restante(), 2);
    assert!(orc.gastar(2));
    assert_eq!(orc.restante(), 0);
}
fn cel(texto: &str) -> Celula {
    Celula { texto: texto.into(), numerica: false }
}
#[test]
fn registrar_mantem_uma_celula_por_coluna() {
    let mut linha = Linha::default();
    for i in 1..=50 {
        linha.registrar(0, cel(&i.to_string()));
    }
    assert_eq!(linha.celulas.len(), 1);
    assert_eq!(linha.celulas[0].1.texto, "50");
}
#[test]
fn registrar_celula_vazia_apaga_a_anterior() {
    let mut linha = Linha::default();
    linha.registrar(2, cel("x"));
    linha.registrar(2, Celula::default());
    assert!(linha.celulas.is_empty());
}
#[test]
fn celula_repetida_na_mesma_coluna_fica_a_ultima() {
    let p = PastaTeste::com_limites(Limites { max_colunas: 4, ..Limites::default() });
    let celulas: String = (1..=50).map(|i| format!(r#"<c r="A1"><v>{i}</v></c>"#)).collect();
    let a = aba(&format!(r#"<row r="1">{celulas}</row>"#), &p);
    assert_eq!(textos(&a), [["50"]]);
}
#[test]
fn texto_longo_de_celula_e_cortado() {
    let p = PastaTeste::com_limites(Limites { max_celula_chars: 3, ..Limites::default() });
    let longo = "x".repeat(10_000);
    let linhas = format!(r#"<row r="1"><c r="A1" t="inlineStr"><is><t>{longo}</t></is></c><c r="B1" t="str"><v>{longo}</v></c></row>"#);
    assert_eq!(textos(&aba(&linhas, &p)), [["xxx…", "xxx…"]]);
}
#[test]
fn indice_de_texto_fora_da_lista_vira_celula_vazia() {
    let a = aba(r#"<row r="1"><c r="A1" t="s"><v>5</v></c><c r="B1"><v>1</v></c></row>"#, &PastaTeste::com_textos(&["a"]));
    assert_eq!(textos(&a), [["", "1"]]);
}
