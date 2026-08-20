fn is_std_api_handle_type_segments(segments: &[String]) -> bool {
    let name = match segments {
        [name] => name.as_str(),
        [std, api, module, name]
            if std == "std"
                && api == "api"
                && (module == "http"
                    || module == "server"
                    || module == "routing"
                    || module == "query"
                    || module == "form"
                    || module == "multipart"
                    || module == "handler"
                    || module == "cors"
                    || module == "middleware"
                    || module == "validation"
                    || module == "errors"
                    || module == "security"
                    || module == "session"
                    || module == "sse"
                    || module == "websocket"
                    || module == "trace"
                    || module == "oauth") =>
        {
            name.as_str()
        }
        [std, api, db, driver, name]
            if std == "std"
                && api == "api"
                && db == "db"
                && (driver == "sqlite" || driver == "postgres" || driver == "redis") =>
        {
            name.as_str()
        }
        [spectra, std, api, module, name]
            if spectra == "spectra"
                && std == "std"
                && api == "api"
                && (module == "http"
                    || module == "server"
                    || module == "routing"
                    || module == "query"
                    || module == "form"
                    || module == "multipart"
                    || module == "handler"
                    || module == "cors"
                    || module == "middleware"
                    || module == "validation"
                    || module == "errors"
                    || module == "security"
                    || module == "session"
                    || module == "sse"
                    || module == "websocket"
                    || module == "trace"
                    || module == "oauth") =>
        {
            name.as_str()
        }
        [spectra, std, api, db, driver, name]
            if spectra == "spectra"
                && std == "std"
                && api == "api"
                && db == "db"
                && (driver == "sqlite" || driver == "postgres" || driver == "redis") =>
        {
            name.as_str()
        }
        _ => return false,
    };
    matches!(
        name,
        "Request"
            | "Response"
            | "Header"
            | "Headers"
            | "Cookie"
            | "Body"
            | "Method"
            | "Status"
            | "Server"
            | "Route"
            | "Router"
            | "RouteMatch"
            | "Query"
            | "QuerySchema"
            | "QueryBinding"
            | "Form"
            | "FormSchema"
            | "FormBinding"
            | "Multipart"
            | "MultipartPart"
            | "HandlerHandle"
            | "AsyncHandlerHandle"
            | "HandlerError"
            | "MiddlewareChain"
            | "MiddlewareHandle"
            | "AsyncMiddlewareHandle"
            | "MiddlewareTrace"
            | "ValidationSchema"
            | "ValidationResult"
            | "ApiError"
            | "CsrfPolicy"
            | "SsrfPolicy"
            | "SessionStore"
            | "Session"
            | "SseServer"
            | "SseConnection"
            | "SseEvent"
            | "WebSocketServer"
            | "WebSocketClient"
            | "WebSocket"
            | "WebSocketMessage"
            | "TraceConfig"
            | "TraceSpan"
            | "CorsPolicy"
            | "OAuthClient"
            | "OAuthToken"
            | "SqliteConnection"
            | "SqliteStatement"
            | "PostgresConnection"
            | "PostgresStatement"
            | "PostgresNotificationChannel"
            | "PostgresNotification"
            | "RedisConnection"
    )
}

fn is_std_api_handle_type_name(name: &str) -> bool {
    matches!(
        name,
        "Request"
            | "Response"
            | "Header"
            | "Headers"
            | "Cookie"
            | "Body"
            | "Method"
            | "Status"
            | "Route"
            | "Router"
            | "RouteMatch"
            | "Query"
            | "QuerySchema"
            | "QueryBinding"
            | "Form"
            | "FormSchema"
            | "FormBinding"
            | "Multipart"
            | "MultipartPart"
            | "HandlerHandle"
            | "AsyncHandlerHandle"
            | "HandlerError"
            | "MiddlewareChain"
            | "MiddlewareHandle"
            | "AsyncMiddlewareHandle"
            | "MiddlewareTrace"
            | "ValidationSchema"
            | "ValidationResult"
            | "ApiError"
            | "CsrfPolicy"
            | "SsrfPolicy"
            | "SessionStore"
            | "Session"
            | "SseServer"
            | "SseConnection"
            | "SseEvent"
            | "WebSocketServer"
            | "WebSocketClient"
            | "WebSocket"
            | "WebSocketMessage"
            | "TraceConfig"
            | "TraceSpan"
            | "CorsPolicy"
            | "OAuthClient"
            | "OAuthToken"
            | "SqliteConnection"
            | "SqliteStatement"
            | "PostgresConnection"
            | "PostgresStatement"
            | "PostgresNotificationChannel"
            | "PostgresNotification"
            | "RedisConnection"
    )
}

fn host_int(runtime_name: &'static str) -> HostFunctionDescriptor {
    HostFunctionDescriptor {
        runtime_name,
        return_type: IRType::Int,
        returns_value: true,
    }
}

fn host_task_int(runtime_name: &'static str) -> HostFunctionDescriptor {
    HostFunctionDescriptor {
        runtime_name,
        return_type: IRType::Task {
            output: Box::new(IRType::Int),
        },
        returns_value: true,
    }
}

fn host_task_bool(runtime_name: &'static str) -> HostFunctionDescriptor {
    HostFunctionDescriptor {
        runtime_name,
        return_type: IRType::Task {
            output: Box::new(IRType::Bool),
        },
        returns_value: true,
    }
}

fn host_task_string(runtime_name: &'static str) -> HostFunctionDescriptor {
    HostFunctionDescriptor {
        runtime_name,
        return_type: IRType::Task {
            output: Box::new(IRType::String),
        },
        returns_value: true,
    }
}

fn host_float(runtime_name: &'static str) -> HostFunctionDescriptor {
    HostFunctionDescriptor {
        runtime_name,
        return_type: IRType::Float,
        returns_value: true,
    }
}

fn host_bool(runtime_name: &'static str) -> HostFunctionDescriptor {
    HostFunctionDescriptor {
        runtime_name,
        return_type: IRType::Bool,
        returns_value: true,
    }
}

fn host_string(runtime_name: &'static str) -> HostFunctionDescriptor {
    HostFunctionDescriptor {
        runtime_name,
        return_type: IRType::String,
        returns_value: true,
    }
}

fn host_void(runtime_name: &'static str) -> HostFunctionDescriptor {
    HostFunctionDescriptor {
        runtime_name,
        return_type: IRType::Void,
        returns_value: false,
    }
}

fn host_tensor_rank0(runtime_name: &'static str) -> HostFunctionDescriptor {
    HostFunctionDescriptor {
        runtime_name,
        return_type: IRType::Tensor {
            dtype: Box::new(IRType::Float),
            rank: Some(0),
            dims: None,
            layout: None,
            device: None,
        },
        returns_value: true,
    }
}

fn host_tensor_dynamic(runtime_name: &'static str) -> HostFunctionDescriptor {
    HostFunctionDescriptor {
        runtime_name,
        return_type: IRType::Tensor {
            dtype: Box::new(IRType::Float),
            rank: None,
            dims: None,
            layout: None,
            device: None,
        },
        returns_value: true,
    }
}

#[derive(Debug, Default)]
struct LoweredTensorMetadata {
    rank: Option<usize>,
    dims: Option<Vec<Option<usize>>>,
    layout: Option<String>,
    device: Option<String>,
}

fn tensor_metadata(type_args: &[TypeAnnotation]) -> LoweredTensorMetadata {
    let mut meta = LoweredTensorMetadata::default();
    let mut dims = Vec::new();

    for ann in type_args {
        let TypeAnnotationKind::Simple { segments } = &ann.kind else {
            continue;
        };
        if segments.len() != 1 {
            continue;
        }
        let name = segments[0].clone();

        if let Some(rank) = name
            .strip_prefix("rank")
            .and_then(|raw| raw.parse::<usize>().ok())
        {
            meta.rank = Some(rank);
            continue;
        }
        if let Some(dim) = name
            .strip_prefix("dim")
            .and_then(|raw| raw.parse::<usize>().ok())
        {
            dims.push(Some(dim));
            continue;
        }
        match name.as_str() {
            "dyn" | "dynamic_dim" | "dim_dynamic" => dims.push(None),
            "dynamic" => meta.rank = None,
            "row_major" | "col_major" | "contiguous" | "strided" => meta.layout = Some(name),
            "cpu" | "wgpu" | "cuda" | "rocm" | "metal" | "directml" | "vulkan" => {
                meta.device = Some(name)
            }
            _ => {}
        }
    }

    if !dims.is_empty() {
        if meta.rank.is_none() {
            meta.rank = Some(dims.len());
        }
        meta.dims = Some(dims);
    }

    meta
}
