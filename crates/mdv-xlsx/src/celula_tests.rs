use crate::celula::interpretar;
use crate::estilos::Estilos;
use crate::limites::Limites;
use crate::modelo::Celula;
use crate::planilha::ContextoPasta;
use crate::Unlimited;

fn texto(c: &Celula) -> &str {
    &c.texto
}

/// Dono dos dados que o `ContextoPasta` só empresta; o estilo 1 é o formato de porcentagem (`numFmtId` 9).
struct Pasta {
    textos: Vec<String>,
    estilos: Estilos,
    limites: Limites,
}

impl Pasta {
    fn nova(textos: &[&str]) -> Pasta {
        let xml = r#"<styleSheet><cellXfs><xf numFmtId="0"/><xf numFmtId="9"/></cellXfs></styleSheet>"#;
        Pasta {
            textos: textos.iter().map(|t| t.to_string()).collect(),
            estilos: Estilos::carregar(xml.as_bytes(), &Unlimited).unwrap(),
            limites: Limites::default(),
        }
    }

    fn ctx(&self) -> ContextoPasta<'_> {
        ContextoPasta { textos: &self.textos, estilos: &self.estilos, data_1904: false, limites: &self.limites }
    }

    /// `interpretar` com o estilo Geral (0).
    fn geral(&self, tipo: Option<&str>, valor: &str) -> Celula {
        interpretar(tipo, valor, 0, &self.ctx())
    }
}

#[test]
fn tipo_s_usa_o_texto_compartilhado() {
    let pasta = Pasta::nova(&["a", "b"]);
    let c = pasta.geral(Some("s"), "1");
    assert_eq!((texto(&c), c.numerica), ("b", false));
}
#[test]
fn tipo_s_fora_do_intervalo_vira_vazio() {
    let pasta = Pasta::nova(&["a"]);
    let c = pasta.geral(Some("s"), "9");
    assert_eq!(texto(&c), "");
}
#[test]
fn tipo_str_e_texto_cru() {
    let c = Pasta::nova(&[]).geral(Some("str"), "abc");
    assert_eq!((texto(&c), c.numerica), ("abc", false));
}
#[test]
fn tipo_b_vira_verdadeiro_ou_falso() {
    assert_eq!(texto(&Pasta::nova(&[]).geral(Some("b"), "1")), "VERDADEIRO");
    assert_eq!(texto(&Pasta::nova(&[]).geral(Some("b"), "0")), "FALSO");
}
#[test]
fn tipo_e_mostra_o_erro() {
    let c = Pasta::nova(&[]).geral(Some("e"), "#DIV/0!");
    assert_eq!((texto(&c), c.numerica), ("#DIV/0!", false));
}
#[test]
fn tipo_d_fica_como_esta() {
    assert_eq!(texto(&Pasta::nova(&[]).geral(Some("d"), "2024-01-02")), "2024-01-02");
}
#[test]
fn tipo_inline_str_fica_como_esta() {
    assert_eq!(texto(&Pasta::nova(&[]).geral(Some("inlineStr"), "ola")), "ola");
}
#[test]
fn tipo_n_usa_o_formato_e_e_numerica() {
    let c = Pasta::nova(&[]).geral(Some("n"), "0.30000000000000004");
    assert_eq!((texto(&c), c.numerica), ("0.3", true));
    let pasta = Pasta::nova(&[]);
    assert_eq!(texto(&interpretar(None, "0.5", 1, &pasta.ctx())), "50%");
}
#[test]
fn valor_vazio_vira_celula_vazia() {
    let c = Pasta::nova(&[]).geral(Some("n"), "");
    assert_eq!(c, Celula::default());
}
#[test]
fn numero_invalido_mostra_o_texto() {
    let c = Pasta::nova(&[]).geral(None, "abc");
    assert_eq!((texto(&c), c.numerica), ("abc", false));
}
