//! Montador de ZIP para testes; só existe em testes (ou com a feature `testutil`).

use flate2::write::DeflateEncoder;
use flate2::Compression;
use std::io::Write;

/// Monta um ZIP mínimo com as entradas dadas; `deflate` comprime as não vazias.
pub fn zip(entradas: &[(&str, &[u8])], deflate: bool) -> Vec<u8> {
    montar(entradas, deflate, &[])
}

/// Monta um ZIP sem compressão com `comentario` no fim (EOCD), para testar a busca do diretório.
pub fn zip_com_comentario(entradas: &[(&str, &[u8])], comentario: &[u8]) -> Vec<u8> {
    montar(entradas, false, comentario)
}

/// Monta o ZIP inteiro: cabeçalhos locais, diretório central e EOCD com `comentario`.
fn montar(entradas: &[(&str, &[u8])], deflate: bool, comentario: &[u8]) -> Vec<u8> {
    let mut saida = Vec::new();
    let mut central = Vec::new();
    for (nome, dados) in entradas {
        let offset = saida.len() as u32;
        let (metodo, corpo) = if deflate && !dados.is_empty() {
            (8u16, comprimir(dados))
        } else {
            (0u16, dados.to_vec())
        };
        let (tam_comp, tam_orig) = (corpo.len() as u32, dados.len() as u32);
        saida.extend_from_slice(b"PK\x03\x04");
        for x in [20u16, 0, metodo, 0, 0] {
            saida.extend_from_slice(&x.to_le_bytes());
        }
        saida.extend_from_slice(&0u32.to_le_bytes());
        saida.extend_from_slice(&tam_comp.to_le_bytes());
        saida.extend_from_slice(&tam_orig.to_le_bytes());
        saida.extend_from_slice(&(nome.len() as u16).to_le_bytes());
        saida.extend_from_slice(&0u16.to_le_bytes());
        saida.extend_from_slice(nome.as_bytes());
        saida.extend_from_slice(&corpo);

        central.extend_from_slice(b"PK\x01\x02");
        for x in [20u16, 20, 0, metodo, 0, 0] {
            central.extend_from_slice(&x.to_le_bytes());
        }
        central.extend_from_slice(&0u32.to_le_bytes());
        central.extend_from_slice(&tam_comp.to_le_bytes());
        central.extend_from_slice(&tam_orig.to_le_bytes());
        central.extend_from_slice(&(nome.len() as u16).to_le_bytes());
        for x in [0u16, 0, 0, 0] {
            central.extend_from_slice(&x.to_le_bytes());
        }
        central.extend_from_slice(&0u32.to_le_bytes());
        central.extend_from_slice(&offset.to_le_bytes());
        central.extend_from_slice(nome.as_bytes());
    }
    let inicio_central = saida.len() as u32;
    let tam_central = central.len() as u32;
    let total = entradas.len() as u16;
    saida.extend_from_slice(&central);
    saida.extend_from_slice(b"PK\x05\x06");
    for x in [0u16, 0, total, total] {
        saida.extend_from_slice(&x.to_le_bytes());
    }
    saida.extend_from_slice(&tam_central.to_le_bytes());
    saida.extend_from_slice(&inicio_central.to_le_bytes());
    saida.extend_from_slice(&(comentario.len() as u16).to_le_bytes());
    saida.extend_from_slice(comentario);
    saida
}

/// Deflate cru (sem cabeçalho zlib), como o ZIP exige para o método 8.
fn comprimir(dados: &[u8]) -> Vec<u8> {
    let mut enc = DeflateEncoder::new(Vec::new(), Compression::default());
    enc.write_all(dados).expect("deflate em memória");
    enc.finish().expect("deflate em memória")
}

const NS_SHEET: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
const NS_REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const NS_PKG: &str = "http://schemas.openxmlformats.org/package/2006/relationships";

/// Aba montada pelo `XlsxBuilder`: nome, conteúdo cru de `<sheetData>` e se está oculta.
struct AbaTeste {
    nome: String,
    linhas: String,
    oculta: bool,
}

/// Montador de `.xlsx` mínimo para testes: abas com `<sheetData>` cru, textos compartilhados,
/// estilos e a flag de data de 1904. Os textos são escapados; o `<sheetData>` é XML do chamador.
#[derive(Default)]
pub struct XlsxBuilder {
    abas: Vec<AbaTeste>,
    textos: Option<Vec<String>>,
    estilos: Option<String>,
    data_1904: bool,
    /// Índices (base 0) das abas cuja parte `sheetN.xml` não vai para o ZIP.
    sem_parte: Vec<usize>,
}

impl XlsxBuilder {
    /// Pasta sem abas, textos nem estilos.
    pub fn novo() -> Self {
        Self::default()
    }

    /// Acrescenta uma aba visível; `linhas_xml` vai dentro de `<sheetData>`.
    pub fn aba(self, nome: &str, linhas_xml: &str) -> Self {
        self.com_aba(nome, linhas_xml, false)
    }

    /// Acrescenta uma aba com `state="hidden"`.
    pub fn aba_oculta(self, nome: &str, linhas_xml: &str) -> Self {
        self.com_aba(nome, linhas_xml, true)
    }

    fn com_aba(mut self, nome: &str, linhas_xml: &str, oculta: bool) -> Self {
        self.abas.push(AbaTeste { nome: nome.to_string(), linhas: linhas_xml.to_string(), oculta });
        self
    }

    /// Faz a aba de índice `i` (base 0) continuar declarada na pasta, mas sem a parte `sheetN.xml` no ZIP.
    pub fn sem_parte_da_aba(mut self, i: usize) -> Self {
        self.sem_parte.push(i);
        self
    }

    /// Define `xl/sharedStrings.xml` com estes textos, na ordem dada.
    pub fn textos(mut self, textos: &[&str]) -> Self {
        self.textos = Some(textos.iter().map(|t| t.to_string()).collect());
        self
    }

    /// Define `xl/styles.xml`; `xml_filhos` são os filhos de `<styleSheet>`.
    pub fn estilos(mut self, xml_filhos: &str) -> Self {
        self.estilos = Some(xml_filhos.to_string());
        self
    }

    /// Liga o sistema de datas de 1904 em `workbookPr`.
    pub fn data_1904(mut self) -> Self {
        self.data_1904 = true;
        self
    }

    /// Monta o `.xlsx` inteiro como ZIP com compressão.
    pub fn construir(&self) -> Vec<u8> {
        let mut arquivos: Vec<(String, String)> = vec![
            ("[Content_Types].xml".into(), r#"<?xml version="1.0" encoding="UTF-8"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"/>"#.into()),
            ("_rels/.rels".into(), self.rels_raiz()),
            ("xl/workbook.xml".into(), self.workbook()),
            ("xl/_rels/workbook.xml.rels".into(), self.rels_workbook()),
        ];
        for (i, aba) in self.abas.iter().enumerate().filter(|(i, _)| !self.sem_parte.contains(i)) {
            let xml = format!(r#"<?xml version="1.0" encoding="UTF-8"?><worksheet xmlns="{NS_SHEET}"><sheetData>{}</sheetData></worksheet>"#, aba.linhas);
            arquivos.push((format!("xl/worksheets/sheet{}.xml", i + 1), xml));
        }
        if let Some(textos) = &self.textos {
            let itens: String = textos.iter().map(|t| format!("<si><t>{}</t></si>", escapar_xml(t))).collect();
            let xml = format!(r#"<?xml version="1.0" encoding="UTF-8"?><sst xmlns="{NS_SHEET}">{itens}</sst>"#);
            arquivos.push(("xl/sharedStrings.xml".into(), xml));
        }
        if let Some(estilos) = &self.estilos {
            let xml = format!(r#"<?xml version="1.0" encoding="UTF-8"?><styleSheet xmlns="{NS_SHEET}">{estilos}</styleSheet>"#);
            arquivos.push(("xl/styles.xml".into(), xml));
        }
        let refs: Vec<(&str, &[u8])> = arquivos.iter().map(|(n, c)| (n.as_str(), c.as_bytes())).collect();
        zip(&refs, true)
    }

    /// `_rels/.rels`: aponta o `officeDocument` para `xl/workbook.xml`.
    fn rels_raiz(&self) -> String {
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="{NS_PKG}"><Relationship Id="rId1" Type="{NS_REL}/officeDocument" Target="xl/workbook.xml"/></Relationships>"#
        )
    }

    /// `xl/workbook.xml`: `workbookPr` (se 1904) e uma `<sheet>` por aba, com `r:id` = `rIdN`.
    fn workbook(&self) -> String {
        let pr = if self.data_1904 { r#"<workbookPr date1904="1"/>"# } else { "" };
        let mut sheets = String::new();
        for (i, aba) in self.abas.iter().enumerate() {
            let estado = if aba.oculta { r#" state="hidden""# } else { "" };
            let n = i + 1;
            sheets.push_str(&format!(
                r#"<sheet name="{}" sheetId="{n}"{estado} r:id="rId{n}"/>"#,
                escapar_xml(&aba.nome)
            ));
        }
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?><workbook xmlns="{NS_SHEET}" xmlns:r="{NS_REL}">{pr}<sheets>{sheets}</sheets></workbook>"#
        )
    }

    /// `xl/_rels/workbook.xml.rels`: uma relação `worksheet` por aba, mais textos e estilos.
    fn rels_workbook(&self) -> String {
        let mut itens = String::new();
        for i in 0..self.abas.len() {
            let n = i + 1;
            itens.push_str(&format!(
                r#"<Relationship Id="rId{n}" Type="{NS_REL}/worksheet" Target="worksheets/sheet{n}.xml"/>"#
            ));
        }
        if self.textos.is_some() {
            itens.push_str(&format!(r#"<Relationship Id="rIdT" Type="{NS_REL}/sharedStrings" Target="sharedStrings.xml"/>"#));
        }
        if self.estilos.is_some() {
            itens.push_str(&format!(r#"<Relationship Id="rIdE" Type="{NS_REL}/styles" Target="styles.xml"/>"#));
        }
        format!(r#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="{NS_PKG}">{itens}</Relationships>"#)
    }
}

/// Escapa `&`, `<`, `>` e `"` para texto e atributo XML.
fn escapar_xml(texto: &str) -> String {
    texto
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::zip;

    #[test]
    fn zip_comeca_com_assinatura_local() {
        let bytes = zip(&[("a", "x".as_bytes())], false);
        assert!(bytes.starts_with(b"PK\x03\x04"));
    }
}
