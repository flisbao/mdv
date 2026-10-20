# Tabela com células longas

Cada linha tem células que quebram em várias linhas; as réguas entre as linhas
mostram onde uma linha da tabela termina e a próxima começa.

| Serviço | Problema (evidência) | Ação | Economia R$/mês |
|---|---|---|---|
| Faturamento | O consumidor registra 3 linhas por mensagem: "Mensagem recebida" 40,1 GB, "Iniciando ciclo" 35,2 GB e "Fila vazia" 30,7 GB | Rebaixar os 3 logs para DEBUG | 10.500 |
| Faturamento | Idem, tudo em nível Information | Amostrar 10% de Info e manter só Warning ou acima | 38.200 / 42.900 |
| Cadastro | Métricas de sessão do ORM com 150 GB (42%) e 120 milhões de linhas | Desligar o log de estatísticas (logger em WARN) | 14.300 |
| Notificações | 8 logs de ciclo de vida com cerca de 20 GB cada | Rebaixar para DEBUG | 15.900 |
| Pagamentos | "Envio desativado para o sistema X" com 18 GB e 25 milhões de linhas | Remover ou amostrar | 1.800 |
| Estoque | Chamadas HTTP de ida e volta registradas por completo (~55%); não reconferido | `logging.level.http=WARN` | ~2.200 |
| Portal | Consulta de flags sem cache, ~5 GB; não conferido | Cache com TTL de 5 a 10 minutos | ~500 |
| Gateway | Log verboso e campo de operação vazio | A validar | — |

## Sem quebra (continua compacto)

| Nome | Valor |
|---|---|
| a | 1 |
| b | 2 |
