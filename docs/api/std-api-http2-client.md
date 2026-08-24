# HTTP/2 client transport

R-2405 adds the native `Http2Client` transport in
`packages/spectra-api/src/http2.rs`. It uses the `h2` state machine and keeps
one connection alive for independent concurrent streams.

```rust
let client = Http2Client::connect(
    "https://api.example.test",
    Http2ClientConfig::default(),
).await?;

let response = client
    .request(Http2Request {
        method: "GET".to_string(),
        target: "/health".to_string(),
        headers: Vec::new(),
        body: Vec::new(),
    })
    .await?;
```

The client accepts `http://` and `https://` endpoints. HTTPS uses rustls,
SNI, WebPKI roots by default, and requires ALPN `h2`; callers that use a
private or test certificate can provide `Http2ClientConfig::with_tls_config`
with an explicit root store. DNS results are checked against the shared SSRF
policy before a socket is opened. Private and link-local addresses are
rejected by default; `allow_private_networks(true)` is an explicit local
testing or trusted-network opt-in.

`Http2ClientConfig` bounds request/response bodies, concurrent streams, header
lists, connection setup, and request duration. Request bodies are sent through
the h2 stream flow-control window in bounded chunks, and received capacity is
released as the body is consumed. The client reuses one connection for
concurrent requests and h2 performs the protocol stream scheduling.

Server push can be received with a callback:

```rust
client.set_push_callback(Some(Arc::new(|push| {
    println!("pushed {}", push.request.target);
})));
```

The promised request must be safe/cacheable according to HTTP/2. Native h2
servers must include an absolute `http://` or `https://` URI in the promise so
the required `:scheme`, `:authority`, and `:path` pseudo-headers can be
validated by the peer.

This item is currently transport-level. The existing `std.api.client.request`
contract remains the HTTP/1.1 client surface; a typed Spectra-language HTTP/2
client and an HTTP/2-aware routed callback adapter need a separate public
contract. Local native evidence covers plain h2 multiplexing, HTTPS ALPN and
explicit trust roots, flow-controlled requests, and server-push callbacks.
The ignored interoperability test also passed against
`https://nghttp2.org` on 2026-08-20; revalidation remains a release gate.

Evidence is reproducible with `python scripts/validate_r2405_http2_client.py`.
The ignored interoperability test can be run in PowerShell with:

```powershell
$env:SPECTRA_HTTP2_EXTERNAL_URL = "https://..."
cargo test -p spectra-api --lib http2::tests::known_external_http2_endpoint_round_trips -- --ignored
```

when a known external h2 endpoint is provisioned.
