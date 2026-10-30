//! Montador de `.docx` e de ZIP para testes; só existe em testes (ou com a feature `testutil`).

use flate2::write::DeflateEncoder;
use flate2::Compression;
use std::io::Write;

/// Cabeçalho `xmlns` usado nas raízes das partes do `.docx` de teste.
pub const NS: &str = r#"xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006""#;

/// Monta um ZIP mínimo com as entradas dadas; `deflate` comprime as não vazias.
pub fn zip(entradas: &[(&str, &[u8])], deflate: bool) -> Vec<u8> {
    montar(entradas, deflate, &[])
}

/// Monta um ZIP sem compressão com `comentario` no fim (EOCD), para testar a busca do diretório.
pub fn zip_com_comentario(entradas: &[(&str, &[u8])], comentario: &[u8]) -> Vec<u8> {
    montar(entradas, false, comentario)
}

/// Montador de `.docx` de teste: partes opcionais entram só se informadas.
#[derive(Default)]
pub struct DocxBuilder {
    corpo: String,
    estilos: Option<String>,
    numeracao: Option<String>,
    notas: Option<String>,
    fim: Option<String>,
    relacoes: Vec<(String, String, String, bool)>,
    comprimido: bool,
}

impl DocxBuilder {
    /// Começa um `.docx` vazio, sem partes opcionais.
    pub fn novo() -> Self {
        Self::default()
    }

    /// Define o conteúdo de `word/document.xml` (filhos de `w:body`).
    pub fn corpo(mut self, xml: &str) -> Self {
        self.corpo = xml.to_string();
        self
    }

    /// Inclui `word/styles.xml` com estes filhos.
    pub fn estilos(mut self, xml: &str) -> Self {
        self.estilos = Some(xml.to_string());
        self
    }

    /// Inclui `word/numbering.xml` com estes filhos.
    pub fn numeracao(mut self, xml: &str) -> Self {
        self.numeracao = Some(xml.to_string());
        self
    }

    /// Inclui `word/footnotes.xml` com estes filhos.
    pub fn notas(mut self, xml: &str) -> Self {
        self.notas = Some(xml.to_string());
        self
    }

    /// Inclui `word/endnotes.xml` com estes filhos.
    pub fn fim(mut self, xml: &str) -> Self {
        self.fim = Some(xml.to_string());
        self
    }

    /// Acrescenta uma relação em `word/_rels/document.xml.rels`; `externo` marca `TargetMode`.
    pub fn relacao(mut self, id: &str, tipo: &str, alvo: &str, externo: bool) -> Self {
        self.relacoes.push((id.to_string(), tipo.to_string(), alvo.to_string(), externo));
        self
    }

    /// Comprime as partes com deflate em vez de guardá-las sem compressão.
    pub fn comprimido(mut self) -> Self {
        self.comprimido = true;
        self
    }

    /// Gera os bytes do ZIP do `.docx`.
    pub fn construir(self) -> Vec<u8> {
        let xml_doc = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?><w:document {NS}><w:body>{}</w:body></w:document>"#,
            self.corpo
        );
        let mut entradas: Vec<(String, Vec<u8>)> = vec![
            ("[Content_Types].xml".to_string(), CONTENT_TYPES.as_bytes().to_vec()),
            ("_rels/.rels".to_string(), RELS_RAIZ.as_bytes().to_vec()),
            ("word/document.xml".to_string(), xml_doc.into_bytes()),
        ];
        let partes = [
            ("word/styles.xml", "styles", &self.estilos, "w:styles"),
            ("word/numbering.xml", "numbering", &self.numeracao, "w:numbering"),
            ("word/footnotes.xml", "footnotes", &self.notas, "w:footnotes"),
            ("word/endnotes.xml", "endnotes", &self.fim, "w:endnotes"),
        ];
        let mut relacoes = String::new();
        for (caminho, tipo, conteudo, raiz) in partes {
            if let Some(filhos) = conteudo {
                let xml = format!(r#"<?xml version="1.0" encoding="UTF-8"?><{raiz} {NS}>{filhos}</{raiz}>"#);
                entradas.push((caminho.to_string(), xml.into_bytes()));
                relacoes.push_str(&relacao_xml(&format!("rId_{tipo}"), tipo, caminho.trim_start_matches("word/"), false));
            }
        }
        for (id, tipo, alvo, externo) in &self.relacoes {
            relacoes.push_str(&relacao_xml(id, tipo, alvo, *externo));
        }
        let rels_doc = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">{relacoes}</Relationships>"#
        );
        entradas.push(("word/_rels/document.xml.rels".to_string(), rels_doc.into_bytes()));
        let refs: Vec<(&str, &[u8])> = entradas.iter().map(|(n, d)| (n.as_str(), d.as_slice())).collect();
        zip(&refs, self.comprimido)
    }
}

const CONTENT_TYPES: &str = r#"<?xml version="1.0" encoding="UTF-8"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>"#;

const RELS_RAIZ: &str = r#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#;

/// Uma `<Relationship>` de `document.xml.rels`; `tipo` é o sufixo após `.../relationships/`.
fn relacao_xml(id: &str, tipo: &str, alvo: &str, externo: bool) -> String {
    let modo = if externo { r#" TargetMode="External""# } else { "" };
    format!(
        r#"<Relationship Id="{id}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/{tipo}" Target="{alvo}"{modo}/>"#
    )
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

#[cfg(test)]
mod tests {
    use super::{zip, DocxBuilder};

    #[test]
    fn zip_comeca_com_assinatura_local() {
        let bytes = zip(&[("a", "x".as_bytes())], false);
        assert!(bytes.starts_with(b"PK\x03\x04"));
    }

    #[test]
    fn builder_gera_zip_com_documento() {
        let bytes = DocxBuilder::novo().corpo("<w:p/>").construir();
        assert!(bytes.windows(b"word/document.xml".len()).any(|w| w == b"word/document.xml"));
    }
}
