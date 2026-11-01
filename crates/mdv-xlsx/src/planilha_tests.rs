use crate::estilos::Estilos;
use crate::limites::Limites;
use crate::celula::Celula;
use crate::planilha::{guardar, ler, Aba};
use crate::Unlimited;

/// Monta a aba a partir das linhas de `sheetData`, com os tetos dados.
fn aba_com(l: &Limites, linhas: &str) -> Aba {
    let xml = format!("<worksheet><sheetData>{linhas}</sheetData></worksheet>");
    let mut orcamento = usize::MAX;
    ler(xml.as_bytes(), &[], &Estilos::vazio(), false, l, &mut orcamento, &Unlimited).unwrap()
}

fn aba(linhas: &str, textos: &[&str], estilos: Option<&str>) -> Aba {
    let xml = format!("<worksheet><sheetData>{linhas}</sheetData></worksheet>");
    let textos: Vec<String> = textos.iter().map(|t| t.to_string()).collect();
    let estilos = match estilos {
        Some(est) => Estilos::carregar(format!("<styleSheet>{est}</styleSheet>").as_bytes(), &Unlimited).unwrap(),
        None => Estilos::vazio(),
    };
    let mut orcamento = usize::MAX;
    let l = Limites::default();
    ler(xml.as_bytes(), &textos, &estilos, false, &l, &mut orcamento, &Unlimited).unwrap()
}

fn ler_com_orcamento(linhas: &str, orcamento: &mut usize) -> Aba {
    let xml = format!("<worksheet><sheetData>{linhas}</sheetData></worksheet>");
    ler(xml.as_bytes(), &[], &Estilos::vazio(), false, &Limites::default(), orcamento, &Unlimited).unwrap()
}

fn textos(a: &Aba) -> Vec<Vec<&str>> {
    a.linhas.iter().map(|l| l.iter().map(|c| c.texto.as_str()).collect()).collect()
}

#[test]
fn formula_mostra_o_valor_em_cache_e_nunca_a_formula() {
    // B3 = SUM(B2:B2) com cache 3; C1 = concatenação com t="str"
    let a = aba(r#"<row r="1"><c r="A1" t="str"><f>"a"&amp;"b"</f><v>ab</v></c></row>
                   <row r="2"><c r="A2"><f>1+2</f><v>3</v></c></row>"#, &[], None);
    assert_eq!(textos(&a), [["ab"], ["3"]]);
}
#[test]
fn formula_sem_valor_em_cache_vira_celula_vazia() {
    let a = aba(r#"<row r="1"><c r="A1"><v>1</v></c><c r="B1"><f>A1+1</f></c></row>"#, &[], None);
    assert_eq!(textos(&a), [["1", ""]]);
}
#[test]
fn tipos_de_celula() {
    // s -> textos[1]; b -> VERDADEIRO/FALSO; e -> texto do erro; inlineStr; d -> ISO como está
    let xml = r#"<row r="1"><c r="A1" t="s"><v>1</v></c><c r="B1" t="b"><v>1</v></c><c r="C1" t="b"><v>0</v></c>
        <c r="D1" t="e"><v>#DIV/0!</v></c><c r="E1" t="inlineStr"><is><t>ola</t></is></c><c r="F1" t="d"><v>2024-01-02</v></c></row>"#;
    assert_eq!(textos(&aba(xml, &["x", "y"], None)), [["y", "VERDADEIRO", "FALSO", "#DIV/0!", "ola", "2024-01-02"]]);
}
#[test]
fn numero_usa_o_formato_do_estilo_e_marca_a_celula_como_numerica() {
    let est = r#"<cellXfs count="2"><xf numFmtId="0"/><xf numFmtId="14"/></cellXfs>"#;
    let a = aba(r#"<row r="1"><c r="A1" s="1"><v>45000</v></c><c r="B1"><v>0.30000000000000004</v></c></row>"#, &[], Some(est));
    assert_eq!(textos(&a), [["2023-03-15", "0.3"]]);
    assert!(a.linhas[0][0].numerica && a.linhas[0][1].numerica);
}
#[test]
fn descarta_linhas_vazias_apara_colunas_e_preenche_buracos() {
    let a = aba(r#"<row r="2"><c r="C2"><v>1</v></c><c r="E2"><v>2</v></c></row><row r="5"/><row r="9"><c r="C9"><v>3</v></c></row>"#, &[], None);
    assert_eq!(textos(&a), [["1", "", "2"], ["3", "", ""]]);
    assert_eq!(a.linhas_total, 2);
}
#[test]
fn coordenada_gigante_nao_aloca_nem_vira_coluna() {
    let l = Limites { max_colunas: 4, ..Limites::default() };
    let a = aba_com(&l, r#"<row r="1048576"><c r="A1048576"><v>1</v></c><c r="XFD1048576"><v>9</v></c></row>"#);
    assert_eq!(textos(&a), [["1"]]);
    assert!(a.colunas_omitidas);
}
#[test]
fn teto_de_linhas_corta_e_conta_o_total() {
    let l = Limites { max_linhas: 2, ..Limites::default() };
    let a = aba_com(&l, r#"<row r="1"><c r="A1"><v>1</v></c></row><row r="2"><c r="A2"><v>2</v></c></row><row r="3"><c r="A3"><v>3</v></c></row>"#);
    assert_eq!((a.linhas.len(), a.linhas_total), (2, 3));
}
#[test]
fn teto_de_celulas_da_grade_corta_linhas() {
    let linhas: String = (1..=5)
        .map(|r| format!(r#"<row r="{r}"><c r="A{r}"><v>1</v></c><c r="B{r}"><v>2</v></c><c r="C{r}"><v>3</v></c></row>"#))
        .collect();
    let a = aba_com(&Limites { max_celulas: 6, ..Limites::default() }, &linhas);
    assert_eq!(a.linhas.len(), 2);
    assert_eq!(a.linhas_total, 5);
    assert!(a.cortada);
    let inteira = aba_com(&Limites { max_celulas: 15, ..Limites::default() }, &linhas);
    assert_eq!(inteira.linhas.len(), 5);
    assert!(!inteira.cortada);
}

#[test]
fn teto_de_bytes_para_a_leitura() {
    let mut orc = 5usize;
    let a = ler_com_orcamento(r#"<row r="1"><c r="A1" t="inlineStr"><is><t>abcdef</t></is></c></row><row r="2"><c r="A2"><v>1</v></c></row>"#, &mut orc);
    assert!(a.cortada);
}
fn cel(texto: &str) -> Celula {
    Celula { texto: texto.into(), numerica: false }
}
#[test]
fn guardar_mantem_uma_celula_por_coluna() {
    let mut celulas = Vec::new();
    for i in 1..=50 {
        guardar(&mut celulas, 0, cel(&i.to_string()));
    }
    assert_eq!(celulas.len(), 1);
    assert_eq!(celulas[0].1.texto, "50");
}
#[test]
fn guardar_celula_vazia_apaga_a_anterior() {
    let mut celulas = Vec::new();
    guardar(&mut celulas, 2, cel("x"));
    guardar(&mut celulas, 2, Celula::default());
    assert!(celulas.is_empty());
}
#[test]
fn celula_repetida_na_mesma_coluna_fica_a_ultima() {
    let l = Limites { max_colunas: 4, ..Limites::default() };
    let celulas: String = (1..=50).map(|i| format!(r#"<c r="A1"><v>{i}</v></c>"#)).collect();
    let a = aba_com(&l, &format!(r#"<row r="1">{celulas}</row>"#));
    assert_eq!(textos(&a), [["50"]]);
}
#[test]
fn texto_longo_de_celula_e_cortado() {
    let l = Limites { max_celula_chars: 3, ..Limites::default() };
    let longo = "x".repeat(10_000);
    let linhas = format!(r#"<row r="1"><c r="A1" t="inlineStr"><is><t>{longo}</t></is></c><c r="B1" t="str"><v>{longo}</v></c></row>"#);
    assert_eq!(textos(&aba_com(&l, &linhas)), [["xxx…", "xxx…"]]);
}
#[test]
fn indice_de_texto_fora_da_lista_vira_celula_vazia() {
    let a = aba(r#"<row r="1"><c r="A1" t="s"><v>5</v></c><c r="B1"><v>1</v></c></row>"#, &["a"], None);
    assert_eq!(textos(&a), [["", "1"]]);
}
