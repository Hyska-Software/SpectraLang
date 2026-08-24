# `std.api.websocket`

`std.api.websocket` fornece um servidor WebSocket dedicado baseado em TCP e
RFC 6455. A troca HTTP/1.1 para `101 Switching Protocols`, validação de
frames, fragmentação, texto UTF-8, ping/pong, close codes e
`permessage-deflate` e o cabeçalho `Sec-WebSocket-Accept` acontecem no host
Rust; o programa Spectra recebe uma superfície tipada e assíncrona.

## Superfície pública

```spectra
from std.api.routing import Route, Router, get, router_new
from std.api.websocket import (
    WebSocketServer, WebSocket, WebSocketMessage,
    server_new, server_listen, server_local_port,
    server_route, server_set_per_message_deflate, server_set_max_message_bytes,
    server_accept, connection_peer_port, connection_receive,
    connection_send_text, connection_send_binary_base64,
    connection_ping, connection_close,
    message_kind, message_len, message_text, message_base64, message_release,
)
```

O fluxo mínimo é criar o servidor, configurar limites, chamar
`server_listen(server, 0)` para uma porta efêmera ou informar uma porta fixa e
aguardar `server_accept(server)`. A aceitação e todas as operações de I/O
retornam `Task<...>` e devem ser consumidas com `await` ou `block_on`:

```spectra
async func echo_once(server: WebSocketServer) returns int {
    let socket: WebSocket = await server_accept(server)
    let message: WebSocketMessage = await connection_receive(socket)
    if message_kind(message) == 1 {
        await connection_send_text(socket, message_text(message))
    }
    message_release(message)
    await connection_close(socket, 1000, "done")
    return connection_peer_port(socket)
}
```

Para integrar o WebSocket ao roteador HTTP, crie uma rota `GET`, associe-a com
`server_route` e entregue o mesmo `Router` a `std.api.server.serve`. Nesse modo
`server_listen` não deve ser chamado nesse `WebSocketServer`; o listener é o
`std.api.server` e `server_accept` recebe as conexões depois do upgrade:

```spectra
let router: Router = router_new()
let socket_route: Route = get(router, "/socket")
let websocket: WebSocketServer = server_new()
server_route(websocket, socket_route)
let http: Server = server_new()
await serve(http, router)
let socket: WebSocket = await server_accept(websocket)
```

Somente rotas `GET` podem ser associadas. Uma requisição sem os cabeçalhos de
upgrade recebe `426 Upgrade Required`; o handshake `101` é processado pelo
worker limitado do host e os bytes que chegaram junto com o cabeçalho são
preservados para o codec RFC 6455.

`message_kind` retorna `1` para texto e `2` para binário. A representação
binária da linguagem é base64 (`connection_send_binary_base64` e
`message_base64`) para manter a fronteira de strings UTF-8 explícita.

## Limites e protocolo

- O limite padrão de frame e mensagem é 16 MiB; use
  `server_set_max_message_bytes` para reduzir o limite antes de escutar.
- Frames de controle são obrigatoriamente finais e têm no máximo 125 bytes.
  Ping recebido gera pong automaticamente.
- Mensagens fragmentadas são reagrupadas antes de serem expostas à linguagem;
  texto inválido em UTF-8 e opcodes/RSV reservados são rejeitados.
- Clientes devem enviar frames mascarados; frames enviados pelo servidor não
  são mascarados.
- `server_set_per_message_deflate(server, true)` negocia a extensão somente
  quando o cliente a anuncia, com `no_context_takeover` nos dois sentidos.
- A aceitação, leitura e escrita usam timeouts e tarefas canceláveis do
  runtime. O handle de mensagem deve ser liberado com `message_release` após o
  consumo.

O listener dedicado continua disponível para serviços que não precisam de
roteamento HTTP. A integração por rota HTTP agora é coberta pelo teste nativo
`r2401_routed_websocket_upgrade_round_trips_through_http_server`; o soak de 10 mil conexões
concorrentes passou como gate de release.

## Evidência

Os testes nativos em `packages/spectra-api/src/websocket.rs` cobrem o handshake
do exemplo RFC 6455, mensagens fragmentadas, ping/pong, close validation,
mascaramento de frames de cliente e negociação/round-trip de
`permessage-deflate`. O fixture
`tests/validation/343_api_websocket.spectra` valida a exposição do módulo,
configuração do listener, associação de uma rota HTTP e execução pela CLI. O
teste nativo roteado e o soak de 10 mil conexões passam no gate reproduzível
`scripts/validate_r2401_websocket.py`.

## Cliente WebSocket

O mesmo módulo expõe `WebSocketClient` para conexões `ws://` e `wss://`. O
cliente gera um nonce novo para cada handshake, valida a resposta `101` e
`Sec-WebSocket-Accept`, aplica a política SSRF default-deny depois da
resolução DNS, usa `rustls` com as raízes WebPKI por padrão para `wss://` e
reutiliza o codec de frames do servidor:

```spectra
let client: WebSocketClient = client_new()
client_set_per_message_deflate(client, true)
client_set_reconnect(client, 3, 100)
let socket: WebSocket = await client_connect(client, "ws://127.0.0.1:9000/socket")
await connection_send_text(socket, "hello")
```

`client_set_reconnect(client, attempts, backoff_ms)` limita a dez tentativas e
sessenta segundos de backoff inicial; a espera usa backoff exponencial limitado
e observa cancelamento. `client_allow_private_networks` é necessário para
testes locais e deve continuar desabilitado para destinos externos por
padrão. Para um endpoint seguro, o mesmo fluxo aceita, por exemplo,
`wss://api.example.com/socket`; a validação de certificado e SNI são feitas
pela configuração TLS do projeto. Configurações Rust com raízes adicionais
podem ser injetadas pela API nativa `WebSocketClient::set_tls_config` em
integrações embutidas.

O fixture `tests/validation/344_api_websocket_client.spectra` exercita a
configuração tipada sem abrir uma conexão externa. Os testes nativos cobrem
handshake, round-trip texto/binário, `wss://` com raiz de confiança explícita,
uma segunda conexão após uma tentativa de handshake falha e um round-trip
externo contra `wss://testserver.host/ws/no-subprotocol/echo`. A evidência é
reproduzível com:

```powershell
$env:SPECTRA_WEBSOCKET_EXTERNAL_URL = "wss://testserver.host/ws/no-subprotocol/echo"
python scripts/validate_r2402_websocket_client.py --require-external --external-url $env:SPECTRA_WEBSOCKET_EXTERNAL_URL
```

O endpoint público é usado somente como interoperabilidade de release; os
testes locais permanecem a base determinística do gate normal.
