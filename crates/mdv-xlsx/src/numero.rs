//! Formatação numérica pura: a regra *Geral* e números com casas decimais e separador de milhar.
//! O separador decimal é `.` e o de milhar `,`, porque o `.xlsx` não guarda o idioma do usuário.
//! Os dois trabalham sobre os 15 dígitos significativos do valor (como o Excel), então nada cresce
//! com o tamanho do número: `decimais` é limitado a 15 e o texto de saída fica em poucas centenas de bytes.

pub(crate) const MAX_DECIMAIS: u8 = 15;

/// Regra *Geral*: até 15 dígitos significativos sem zeros à direita; notação científica
/// quando o expoente é 15 ou mais, ou menor que -4. Não finito vira texto vazio.
pub(crate) fn geral(v: f64) -> String {
    if v == 0.0 {
        return "0".to_string();
    }
    let sinal = if v < 0.0 { "-" } else { "" };
    let Some((dig, expo)) = quinze_digitos(v.abs()) else {
        return String::new();
    };
    let digitos = String::from_utf8_lossy(&dig).trim_end_matches('0').to_string();
    if !(-4..15).contains(&expo) {
        let (primeiro, resto) = digitos.split_at(1);
        let resto = if resto.is_empty() { String::new() } else { format!(".{resto}") };
        let sinal_expo = if expo < 0 { '-' } else { '+' };
        return format!("{sinal}{primeiro}{resto}E{sinal_expo}{:02}", expo.abs());
    }
    let (inteiro, fracao) = if expo >= 0 {
        let k = expo as usize + 1;
        let mut t = digitos.clone();
        while t.len() < k {
            t.push('0');
        }
        (t[..k].to_string(), t[k..].to_string())
    } else {
        ("0".to_string(), "0".repeat((-expo - 1) as usize) + &digitos)
    };
    if fracao.is_empty() {
        format!("{sinal}{inteiro}")
    } else {
        format!("{sinal}{inteiro}.{fracao}")
    }
}

/// Número com `decimais` casas (no máximo 15), arredondamento meia para longe de zero sobre os
/// 15 dígitos significativos e separador de milhar opcional. Zero não ganha sinal de menos.
pub(crate) fn fixo(v: f64, decimais: u8, milhar: bool) -> String {
    let d = usize::from(decimais.min(MAX_DECIMAIS));
    let Some((dig, expo)) = quinze_digitos(v.abs()) else {
        return String::new();
    };
    let p = expo + 1;
    let k = p + d as i64;
    let mut n: Vec<u8> = Vec::new();
    if k >= 0 {
        let k = k as usize;
        n.extend((0..k).map(|i| dig.get(i).copied().unwrap_or(b'0')));
        if dig.get(k).is_some_and(|b| *b >= b'5') {
            incrementar(&mut n);
        }
    }
    if n.is_empty() {
        n.push(b'0');
    }
    let (inteiro, fracao) = if n.len() > d {
        let corte = n.len() - d;
        (n[..corte].to_vec(), n[corte..].to_vec())
    } else {
        let mut f = vec![b'0'; d - n.len()];
        f.extend_from_slice(&n);
        (vec![b'0'], f)
    };
    let mut s = String::new();
    if v < 0.0 && n.iter().any(|b| *b != b'0') {
        s.push('-');
    }
    if milhar {
        s.push_str(&agrupar_milhar(&inteiro));
    } else {
        s.extend(inteiro.iter().map(|b| char::from(*b)));
    }
    if d > 0 {
        s.push('.');
        s.extend(fracao.iter().map(|b| char::from(*b)));
    }
    s
}

/// Dígitos da parte inteira com `,` a cada três casas, da direita para a esquerda.
fn agrupar_milhar(inteiro: &[u8]) -> String {
    let tam = inteiro.len();
    let mut s = String::with_capacity(tam + tam / 3);
    for (i, b) in inteiro.iter().enumerate() {
        if i > 0 && (tam - i).is_multiple_of(3) {
            s.push(',');
        }
        s.push(char::from(*b));
    }
    s
}

/// Os 15 dígitos significativos do valor absoluto e o expoente decimal dele; `None` se não for
/// finito. É a única checagem de finitude de `geral` e `fixo` (as bordas são `Celula` e `Formato::aplicar`).
fn quinze_digitos(abs: f64) -> Option<(Vec<u8>, i64)> {
    if !abs.is_finite() {
        return None;
    }
    let texto = format!("{abs:.14e}");
    let (mantissa, expo) = texto.split_once('e')?;
    let dig: Vec<u8> = mantissa.bytes().filter(u8::is_ascii_digit).collect();
    Some((dig, expo.parse().ok()?))
}

/// Soma 1 ao número escrito em dígitos, propagando o vai-um; se estourar, abre um dígito à frente.
fn incrementar(n: &mut Vec<u8>) {
    let mut vai_um = true;
    for b in n.iter_mut().rev() {
        if !vai_um {
            break;
        }
        if *b == b'9' {
            *b = b'0';
        } else {
            *b += 1;
            vai_um = false;
        }
    }
    if vai_um {
        n.insert(0, b'1');
    }
}
