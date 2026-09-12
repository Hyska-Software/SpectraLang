# `spectra.api.db.redis`

R-2507 fornece um driver Redis real para Redis 7, usando conexão externa,
health check `PING`, pool compartilhado e operações assíncronas fora do reactor.

A superfície disponível é `open`, `close`, `get`, `set`, `delete`, `expire`,
`incr` e `exists`. Cada operação bloqueante tem uma variante `*_async`
(`open_async`, `close_async`, `get_async`, `set_async`, `delete_async`,
`exists_async`, `incr_async`, `expire_async`) que roda o comando em uma
thread de I/O dedicada e devolve um `Task<T>` cancelável pelo protocolo
padrão de tasks (`std.async.task.cancel`/`await`/`block_on`). O cancelamento
aborta comandos ainda não despachados; um comando já em voo termina dentro
do `command_timeout` configurado na conexão. As variantes bloqueantes
permanecem disponíveis como compatibilidade documentada para código síncrono.

`last_error_code(connection)` e `last_error_message(connection)` leem o
último erro armazenado no próprio handle da conexão (slot por handle), sem
compartilhamento global entre os drivers sqlite/postgres/redis.

Pub/sub existe no contrato Rust de `spectra-db`; não há
host call de stream até que o protocolo de handles assíncronos da linguagem
possa representar notificações sem uma API incompleta.

A task permanece `in_progress` até a lane Redis 7 produzir o relatório
independente `passed`. Ausência de Redis local gera apenas
`skipped_environment` e não é evidência de produção.

Senhas, URLs completas, chaves e valores não são exportados pelo tracing por
padrão. O backend Redis é o consumidor previsto para R-2417 e R-2513.
