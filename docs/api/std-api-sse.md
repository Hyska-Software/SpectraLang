# `std.api.sse`

`std.api.sse` fornece um transporte HTTP/1.1 dedicado para Server-Sent Events
(SSE). O servidor envia `Content-Type: text/event-stream`, mantém a conexão
aberta e publica registros SSE individuais sem materializar um corpo HTTP
finito.

## Superfície pública

```spectra
from std.api.sse import (
    SseServer, SseConnection, SseEvent,
    server_new, server_response, server_listen, server_local_port,
    server_set_heartbeat_interval, server_set_replay_capacity,
    server_set_max_event_bytes, server_accept, server_publish,
    event_new, event_id, event_type, event_data, event_retry_ms,
    event_release, connection_peer_port, connection_last_event_id,
    connection_send, connection_heartbeat, connection_close,
)
```

Para usar o ciclo de vida normal de `std.api.server`, crie uma resposta SSE
vinculada ao mesmo `SseServer` e registre-a no handler da rota:

```spectra
let stream: SseServer = server_new()
let response: Response = server_response(stream)
let route: int = get(router, "/events")
let handler: HandlerHandle = register_sync(route, response)
```

`server_response` não abre um listener separado. A rota envia o handshake
SSE no loop HTTP compartilhado, registra o cliente como assinante do servidor
e recebe eventos publicados por `server_publish`; heartbeat, replay por
`Last-Event-ID` e limites de fila são aplicados pelo mesmo estado do stream.

`event_new(id, type, data, retry_ms)` usa strings vazias para omitir `id` ou
`event`. O campo `data` é dividido em linhas `data:` conforme o formato SSE;
`retry_ms` é serializado como a dica de reconexão do cliente quando diferente
de zero. `event_release` encerra o ciclo de vida do handle do evento.

```spectra
async func publish_once(server: SseServer) returns int {
    let event: SseEvent = event_new("42", "update", "hello", 1000)
    let delivered: int = await server_publish(server, event)
    event_release(event)
    return delivered
}
```

O servidor deve ser configurado antes de `server_listen`. O heartbeat padrão é
de 15 segundos; `server_set_heartbeat_interval` altera o intervalo antes do
bind. `server_set_replay_capacity` limita a quantidade de eventos retidos e o
log também é limitado a 64 MiB para impedir crescimento de memória sem limite.

## Handshake, heartbeat e resume

`server_accept` aceita `GET` HTTP/1.1 e responde com `200 OK`,
`text/event-stream`, `Cache-Control: no-cache`, conexão persistente e
`X-Accel-Buffering: no`. Se o cliente enviar `Last-Event-ID`, o servidor
reenvia somente os eventos identificados depois desse ID. Se o ID não estiver
no log, reenvia os eventos identificados atualmente retidos; eventos sem ID
não participam do replay.

Além dos heartbeats automáticos (`: heartbeat\n\n`),
`connection_heartbeat` permite emitir um comentário imediatamente. As
operações de escrita e aceitação são tarefas canceláveis e usam timeouts
configurados no host.

## Limites e evidência

- O evento individual tem limite padrão de 1 MiB e o limite configurável não
  pode exceder esse valor.
- IDs e nomes de evento não podem conter CR, LF ou NUL.
- A capacidade de replay é limitada a 65.536 eventos e 64 MiB.
- Os testes nativos em `packages/spectra-api/src/sse.rs` cobrem formato
  multiline, streaming, `retry`, heartbeat automático e resume por
  `Last-Event-ID`.
- O fixture `tests/validation/345_api_sse.spectra` valida a superfície tipada
  pela CLI e o gate reproduzível é
  `scripts/validate_r2403_sse.py`.

A resposta roteada usa o transporte de stream do servidor HTTP sem converter
o evento em um corpo finito: a conexão permanece registrada no loop `mio`,
recebe heartbeats e drena uma fila limitada de eventos publicados.
