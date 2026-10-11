# Blocos de código

Bloco em Rust:

```rust
use std::io::{self, Read};

fn main() -> io::Result<()> {
    let mut entrada = String::new();
    io::stdin().read_to_string(&mut entrada)?;
    println!("{} linhas", entrada.lines().count());
    Ok(())
}
```

Bloco em Python:

```python
def fibonacci(n: int) -> list[int]:
    """Retorna os n primeiros números de Fibonacci."""
    a, b = 0, 1
    out = []
    for _ in range(n):
        out.append(a)
        a, b = b, a + b
    return out

print(fibonacci(10))
```

Bloco em JavaScript:

```javascript
const soma = (...nums) => nums.reduce((a, b) => a + b, 0);
console.log(soma(1, 2, 3)); // 6
```

Bloco em SQL:

```sql
SELECT usuario, COUNT(*) AS total
FROM pedidos
WHERE criado_em >= '2026-01-01'
GROUP BY usuario
ORDER BY total DESC;
```

Bloco em shell:

```bash
for f in *.md; do
  echo "== $f"
  wc -l "$f"
done
```

Bloco sem linguagem:

```
texto simples
    com recuo preservado
```

Código com linha muito longa, para testar a quebra sem scroll horizontal:

```text
Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor incididunt ut labore et dolore magna aliqua. Ut enim ad minim veniam, quis nostrud exercitation ullamco laboris nisi ut aliquip ex ea commodo consequat.
```

Código inline com espaços preservados: `ls   -la    /tmp`.

Item de lista iniciado por bloco de código:

1. Rode o comando:

   ```bash
   cargo test
   ```

2. Confira a saída.
