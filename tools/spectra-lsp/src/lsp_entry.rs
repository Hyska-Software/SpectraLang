#[tokio::main]
async fn main() {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();
    let (service, socket) = LspService::new(|client| Backend {
        client,
        state: Arc::new(BackendState::default()),
    });
    Server::new(stdin, stdout, socket).serve(service).await;
}
