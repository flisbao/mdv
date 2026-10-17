//! Apoio só dos testes da crate.
// Os testes das próximas tarefas usam estas funções; por ora algumas ficam sem uso.
#![allow(dead_code)]

use crate::Budget;

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
