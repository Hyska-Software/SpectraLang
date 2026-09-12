# `std.api.session`

`std.api.session` fornece estado de sessão no servidor. O cookie enviado ao
cliente deve carregar apenas o identificador opaco; o valor da sessão fica no
store configurado. Para proteger o identificador contra adulteração, combine a
API com `std.api.http.cookie_sign`/`cookie_verify` antes de aceitar o valor em
uma requisição.

## Stores

`memory_store()` cria um store concorrente em memória do processo. Para uma
aplicação com múltiplas réplicas, `redis_store(redis_connection, prefix)` usa a
conexão Redis real de `std.api.db.redis` e grava registros JSON com TTL nativo
em milissegundos. O prefixo é validado para impedir caracteres que alterem a
chave Redis.

Ambos implementam o mesmo contrato:

- `create(store, value, ttl_ms, max_lifetime_ms, sliding)` cria um identificador
  aleatório de 256 bits;
- `lookup(store, id)` busca a sessão e, quando `sliding=true`, estende o TTL
  até `max_lifetime_ms` contado desde a criação;
- `revoke(store, id)` remove a sessão imediatamente;
- `is_valid(session)` consulta o backend novamente, portanto uma sessão
  revogada não continua válida só porque um handle antigo existe.

`ttl_ms` deve ser positivo e não pode exceder `max_lifetime_ms`. O payload é
limitado a 1 MiB. Falhas retornam o handle nulo ou `false`; consulte
`error_code()` e `error_message()` para distinguir configuração inválida,
expiração, corrupção e falha de backend.

## Integração com cookies

Um fluxo típico cria a sessão, cria um cookie com o mesmo `id(session)`, define
`HttpOnly`, `Secure`, `SameSite` e `Max-Age`, assina o cookie com um segredo
rotacionável e o anexa com `response_with_cookie`. Na requisição seguinte,
extraia o cookie, reconstrua o objeto Cookie, verifique a assinatura e só então
chame `lookup`.

O fixture executável é
`tests/validation/342_api_session.spectra`. Os testes nativos em
`packages/spectra-api/src/session.rs` cobrem criação, lookup, expiração,
sliding, limite máximo, revogação e o formato persistido pelo backend Redis.
A certificação contra um serviço Redis 7 real permanece no gate de R-2507;
compilar ou pular esse serviço não é tratado como prova de disponibilidade
externa.
