//! Apoio só dos testes da crate.

use std::sync::{Arc, Mutex};

use image::DynamicImage;

use crate::{Budget, PictureProvider};

/// `Budget` esgotado: prova que os laços abortam com erro (sem pânico) quando o limite estoura.
pub struct Exhausted;

impl Budget for Exhausted {
    fn check(&self) -> Result<(), String> {
        Err("limite de memória".to_string())
    }
    fn check_parse(&self, _len: usize) -> Result<(), String> {
        Err("limite de memória".to_string())
    }
}

/// Conteúdo (texto) da entrada `name` de um ZIP sem compressão (o `ZipWriter` nunca comprime).
pub fn zip_entry(zip: &[u8], name: &str) -> String {
    let mut i = 0;
    while i + 30 <= zip.len() {
        if zip[i..i + 4] == [0x50, 0x4b, 0x03, 0x04] {
            let size = u32::from_le_bytes(zip[i + 18..i + 22].try_into().unwrap()) as usize;
            let nlen = u16::from_le_bytes(zip[i + 26..i + 28].try_into().unwrap()) as usize;
            let xlen = u16::from_le_bytes(zip[i + 28..i + 30].try_into().unwrap()) as usize;
            let start = i + 30 + nlen + xlen;
            if &zip[i + 30..i + 30 + nlen] == name.as_bytes() {
                return String::from_utf8(zip[start..start + size].to_vec()).unwrap();
            }
            i = start + size;
        } else {
            i += 1;
        }
    }
    panic!("entrada {name} não encontrada");
}

/// `PictureProvider` de teste: devolve uma imagem lisa (ou um erro) e anota o que foi pedido.
pub struct FakePictures {
    size: (u32, u32),
    error: Option<String>,
    mermaid: Option<Result<(), String>>,
    loads: Mutex<Vec<String>>,
}

impl FakePictures {
    pub fn new() -> Self {
        FakePictures { size: (4, 4), error: None, mermaid: None, loads: Mutex::new(Vec::new()) }
    }
    pub fn failing(msg: &str) -> Self {
        FakePictures { error: Some(msg.to_string()), ..Self::new() }
    }
    pub fn with_size(w: u32, h: u32) -> Self {
        FakePictures { size: (w, h), ..Self::new() }
    }
    /// Liga o Mermaid (desenha sempre com sucesso).
    pub fn with_mermaid(mut self) -> Self {
        self.mermaid = Some(Ok(()));
        self
    }
    pub fn with_failing_mermaid(mut self, msg: &str) -> Self {
        self.mermaid = Some(Err(msg.to_string()));
        self
    }
    /// Caminhos pedidos a `load`, em ordem.
    pub fn loads(&self) -> Vec<String> {
        self.loads.lock().unwrap().clone()
    }
    fn image(&self) -> Arc<DynamicImage> {
        Arc::new(DynamicImage::ImageRgb8(image::RgbImage::new(self.size.0, self.size.1)))
    }
}

impl PictureProvider for FakePictures {
    fn load(&self, src: &str) -> Result<Arc<DynamicImage>, String> {
        self.loads.lock().unwrap().push(src.to_string());
        match &self.error {
            Some(e) => Err(e.clone()),
            None => Ok(self.image()),
        }
    }
    fn mermaid(&self, _source: &str) -> Option<Result<Arc<DynamicImage>, String>> {
        self.mermaid.as_ref().map(|r| r.clone().map(|()| self.image()))
    }
}
