# `std.api.websocket`

`std.api.websocket` fornece um servidor WebSocket dedicado baseado em TCP e
RFC 6455. A troca HTTP/1.1 para `101 Switching Protocols`, validação de
frames, fragmentação, texto UTF-8, ping/pong, close codes e
`permessage-deflate` e o cabeçalho `Sec-WebSocket-Accept` acontecem no host
Rust; o programa Spectra recebe uma superfície tipada e assíncrona.

## Superfície pública

```spectra
from std.api.websocket import (
    WebSocketServer, WebSocket, WebSocketMessage,
    server_new, server_listen, server_local_port,
    server_set_per_message_deflate, server_set_max_message_bytes,
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

O módulo é deliberadamente separado do `std.api.server` HTTP nesta etapa. A
integração de uma rota HTTP que faça o upgrade e o teste de carga de 10 mil conexões
concorrentes ainda são critérios de fechamento do R-2401; portanto o
item permanece `in_progress` mesmo com o núcleo RFC 6455 e a superfície
tipada já implementados.

## Evidência

Os testes nativos em `packages/spectra-api/src/websocket.rs` cobrem o handshake
do exemplo RFC 6455, mensagens fragmentadas, ping/pong, close validation,
mascaramento de frames de cliente e negociação/round-trip de
`permessage-deflate`. O fixture
`tests/validation/343_api_websocket.spectra` valida a exposição do módulo,
configuração do listener e execução pela CLI. O gate reproduzível é
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
handshake, round-trip texto/binário, `wss://` com raiz de confiança explícita e
uma segunda conexão após uma tentativa de handshake falha. A certificação
contra um echo server externo ainda é obrigatória antes de marcar o R-2402 como
completo.
