---
titulo: Resposta de agente
modelo: exemplo
tags: [teste, agente]
---

# Resposta de agente

Resposta final ao usuário, com um fato verificado[^1].

> [!NOTE]
> Alerta de nota no estilo GFM.

> [!WARNING]
> Alerta de aviso, com **formatação** dentro.

<details>
<summary>Detalhes da execução</summary>

- passo 1: leitura do arquivo
- passo 2: validação

</details>

Cerca sem fechar, para testar o comportamento no fim do arquivo:

```rust
fn incompleto() {
    println!("sem fechamento");

[^1]: Nota de rodapé com referência de volta ao texto.
