struct ClientEntry {
    client: std::sync::Arc<HttpClient>,
    timeout_ms: SpectraHostValue,
}

pub extern "C" fn client_new(ctx: *mut SpectraHostCallContext) -> i32 {
    let mut store = store().lock().unwrap_or_else(|e| e.into_inner());
    write_result(
        ctx,
        store.clients.insert(ClientEntry {
            client: std::sync::Arc::new(HttpClient::new(ClientConfig::default())),
            timeout_ms: DEFAULT_TIMEOUT_MS,
        }),
    )
}

pub extern "C" fn client_timeout_ms(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let store = store().lock().unwrap_or_else(|e| e.into_inner());
    let Some(timeout) = store.clients.get(&args[0]).map(|entry| entry.timeout_ms) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    write_result(ctx, timeout)
}

pub extern "C" fn client_set_ssrf_policy(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 2) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(policy) = crate::security::clone_ssrf_policy(args[1]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let store = store().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let Some(entry) = store.clients.get(&args[0]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    entry.client.set_ssrf_policy(policy);
    write_result(ctx, 1)
}

pub extern "C" fn client_request(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 2) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let (client, request) = {
        let store = store().lock().unwrap_or_else(|e| e.into_inner());
        let Some(entry) = store.clients.get(&args[0]) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some(request) = http::clone_request(args[1]) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        (std::sync::Arc::clone(&entry.client), request)
    };

    let request = ClientRequest {
        method: request.method.as_str().to_string(),
        url: request.path,
        headers: request.headers.iter().cloned().collect(),
        body: request.body,
    };
    let task = spectra_runtime::stdlib::spawn_cancellable_io_task_with_token(move |token| {
        let response = client.request_nonblocking(request, token).map_err(|_| ())?;
        store_client_response(response).ok_or(())
    })
    .map_err(|_| HOST_STATUS_INTERNAL_ERROR);
    match task {
        Ok(task) => write_result(ctx, task),
        Err(status) => status,
    }
}

fn store_client_response(response: ClientResponse) -> Option<SpectraHostValue> {
    let status = Status::new(response.status_code).ok()?;
    let mut typed = Response::new(status).with_body(response.body.bytes());
    for header in response.headers {
        typed = typed.with_header(header.name, header.value).ok()?;
    }
    Some(http::store_response(typed))
}

struct ClientStore {
    clients: ApiHandleTable<ClientEntry>,
}

impl ClientStore {
    fn new() -> Self {
        Self {
            clients: ApiHandleTable::new(HandleKind::ApiClient),
        }
    }
}

fn store() -> &'static Mutex<ClientStore> {
    static STORE: OnceLock<Mutex<ClientStore>> = OnceLock::new();
    STORE.get_or_init(|| Mutex::new(ClientStore::new()))
}
