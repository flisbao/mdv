# Diagramas Mermaid para testar o zoom

Amostra com dados fictícios para conferir o zoom de diagramas (`i`). Role até um diagrama,
aperte `i` e use `+`/`-`, `h`/`j`/`k`/`l`, espaço/`b`, `g`/`G` e `0`.

## 1. Diagrama pequeno (cabe na tela)

Serve de referência: deve ficar legível mesmo sem zoom.

```mermaid
sequenceDiagram
    participant U as Usuário
    participant API as loja-api
    participant DB as loja-db

    U->>API: GET /produtos/42
    API->>DB: SELECT produto
    DB-->>API: linha do produto
    API-->>U: 200 OK
```

## 2. Diagrama grande (precisa de zoom)

Checkout de uma loja fictícia: 8 participantes, nomes longos, notas e blocos `alt` aninhados.
Na tela inteira o texto fica ilegível; com `i` dá para aproximar e andar pelo diagrama.

```mermaid
sequenceDiagram
    autonumber
    participant C as App Cliente
    participant GW as checkout-gateway
    participant CAT as catalogo-produtos
    participant EST as controle-estoque
    participant PAG as servico-pagamentos
    participant PED as gestao-pedidos
    participant GTW as Gateway Pagamento X
    participant NOT as disparador-notificacoes

    C->>GW: POST /checkout/pedidos
    Note over C,GW: Header: X-Loja: LOJA_EXEMPLO

    Note over GW: 1. checkout habilitado?
    Note over GW: 2. flag cupom-fidelidade ativa?
    Note over GW: 3. X-Loja = LOJA_EXEMPLO?

    alt cupom informado no request
        Note over GW: Pré-validação do cupom
        GW->>CAT: GET /api/catalogo/cupons/validar
        Note over GW,CAT: codigo=cupom.codigo, canal=clienteId
        CAT-->>GW: cupom válido
        GW->>EST: GET /api/v2/estoque/disponibilidade
        Note over GW,EST: sku=item.sku, quantidade=item.qtd
        EST-->>GW: DisponibilidadeItem (inclui prazoReposicao)
        Note over GW: 404 = ProdutoNaoEncontradoException (422)
        Note over GW: quantidade=0 = ItemSemEstoqueException (409)
        Note over GW: prazoReposicao mapeado para pedido.prazoEntrega
    end

    GW->>PED: POST /api/v1/pedidos
    Note over GW,PED: Header: X-Loja: LOJA_EXEMPLO — repassa cupom (incluindo prazoEntrega)

    Note over PED: CriarPedidoUseCase — ponto de entrada único
    Note over PED: ResolvedorDeProvedorPagamento escolhe o gateway pelo X-Loja
    PED->>GTW: PUT /cobrancas/ref
    GTW-->>PED: ref + urlCobranca

    alt cupom ausente
        PED-->>GW: ref + linkPagamento + status
    else cupom presente
        Note over PED: AplicarCupomAoPedidoUseCase
        PED->>GTW: POST /reservas-desconto
        GTW-->>PED: reservaId + validade
        Note over PED: Valida uso duplicado via CupomRepository
        PED->>GTW: POST /descontos
        Note over PED,GTW: ref + reservaId + cupom + cliente
        GTW-->>PED: idDesconto + status APLICADO
        Note over PED: GeradorLinkPagamento.gerar(urlCobranca, reservaId)
        Note over PED: Persiste PedidoComCupom com fluxo=CHECKOUT_V2 e correlationId
        PED->>PAG: POST /api/v1/links/checkout
        Note over PED,PAG: idDesconto + link + acordo + correlationId + fluxo=CHECKOUT_V2
        alt PAG retorna sucesso
            PAG-->>PED: 201 Created
            PED-->>GW: ref + linkPagamento + status
        else PAG retorna erro
            PAG-->>PED: erro
            Note over PED: Atualiza status do pedido para ERRO_INTERNO
            PED-->>GW: ref + linkPagamento + status
        end
    end

    GW->>NOT: POST /api/v1/notificacoes/pedido-criado
    Note over GW,NOT: Confirmação por e-mail — fluxo existente sem alteração
    GW-->>C: 201 Created
```

## 3. Fluxograma (outro tipo de diagrama)

Confere que o zoom também serve para diagramas que não são de sequência.

```mermaid
flowchart TD
    A[Pedido criado] --> B{Pagamento aprovado?}
    B -->|sim| C[Separar itens]
    B -->|não| D[Cancelar pedido]
    C --> E{Item em estoque?}
    E -->|sim| F[Enviar]
    E -->|não| G[Aguardar reposição]
    G --> E
    F --> H[Entregue]
    D --> I[Notificar cliente]
    H --> I
```
