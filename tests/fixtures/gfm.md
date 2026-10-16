---
titulo: Fixture de golden
tags: [gfm, teste]
---

# Construções que permanecem

> [!NOTE]
> Alerta de nota.

> [!WARNING]
> Alerta de aviso, com **formatação** e `código` dentro.

> [!TIP]
> Dica curta.

## Detalhes e rodapé

<details>
<summary>Ver detalhes da medição</summary>

Foram 10 mil requisições em 5 minutos.

</details>

A taxa de erro caiu para 0,2%[^1].

[^1]: Fonte: painel de observabilidade, semana 41.

## Tarefas

- [x] feito
- [ ] pendente
  - [x] subtarefa feita
  - [ ] subtarefa pendente

## Código e diff

```diff
@@ -1,2 +1,2 @@
-removida
+adicionada
```

```rust
fn main() {
    println!("olá");
}
```

## Tabela com texto longo

| Nome | Descrição |
|------|-----------|
| a | uma descrição bem comprida para forçar o truncamento da célula na largura estreita |
| b | curta |
