fn host_call_args<'a>(
    ctx: *mut SpectraHostCallContext,
    expected_args: usize,
) -> Result<(&'a [SpectraHostValue], &'a mut [SpectraHostValue]), i32> {
    if ctx.is_null() {
        return Err(HOST_STATUS_INVALID_ARGUMENT);
    }

    unsafe {
        let ctx_ref = &mut *ctx;
        if ctx_ref.arg_len != expected_args {
            return Err(HOST_STATUS_INVALID_ARGUMENT);
        }
        if expected_args > 0 && ctx_ref.args.is_null() {
            return Err(HOST_STATUS_INVALID_ARGUMENT);
        }
        if ctx_ref.result_len == 0 || ctx_ref.results.is_null() {
            return Err(HOST_STATUS_INVALID_ARGUMENT);
        }

        let args = if expected_args == 0 {
            &[]
        } else {
            slice::from_raw_parts(ctx_ref.args, ctx_ref.arg_len)
        };
        let results = slice::from_raw_parts_mut(ctx_ref.results, ctx_ref.result_len);
        Ok((args, results))
    }
}

fn host_call_void_args<'a>(
    ctx: *mut SpectraHostCallContext,
    expected_args: usize,
) -> Result<&'a [SpectraHostValue], i32> {
    if ctx.is_null() {
        return Err(HOST_STATUS_INVALID_ARGUMENT);
    }

    unsafe {
        let ctx_ref = &mut *ctx;
        if ctx_ref.arg_len != expected_args {
            return Err(HOST_STATUS_INVALID_ARGUMENT);
        }
        if expected_args > 0 && ctx_ref.args.is_null() {
            return Err(HOST_STATUS_INVALID_ARGUMENT);
        }

        if expected_args == 0 {
            Ok(&[])
        } else {
            Ok(slice::from_raw_parts(ctx_ref.args, ctx_ref.arg_len))
        }
    }
}

#[derive(Clone, Copy)]
struct AsyncTask {
    value: SpectraHostValue,
    cancelled: bool,
    failed: bool,
    completed: bool,
    parent_scope: Option<SpectraHostValue>,
    cancel_handle: SpectraHostValue,
    timeout_inner: Option<SpectraHostValue>,
    deadline_ms: Option<SpectraHostValue>,
    join_order: Option<SpectraHostValue>,
}

struct AsyncScope {
    _parent: Option<SpectraHostValue>,
    child_scopes: Vec<SpectraHostValue>,
    children: Vec<SpectraHostValue>,
    cancelled: bool,
    joined_count: SpectraHostValue,
    failures: SpectraHostValue,
}

#[derive(Clone, Copy)]
enum AsyncStreamKind {
    Source,
    Map {
        upstream: SpectraHostValue,
        op: SpectraHostValue,
        arg: SpectraHostValue,
    },
    Filter {
        upstream: SpectraHostValue,
        predicate: SpectraHostValue,
        arg: SpectraHostValue,
    },
    Take {
        upstream: SpectraHostValue,
        remaining: SpectraHostValue,
    },
    Skip {
        upstream: SpectraHostValue,
        remaining: SpectraHostValue,
    },
    Chunks {
        upstream: SpectraHostValue,
        size: SpectraHostValue,
    },
    Fuse {
        upstream: SpectraHostValue,
        fused_done: bool,
    },
}

struct AsyncStream {
    kind: AsyncStreamKind,
    buffer: VecDeque<SpectraHostValue>,
    capacity: usize,
    pending_next: VecDeque<SpectraHostValue>,
    done: bool,
    cancelled: bool,
    failed: bool,
    last_next_status: SpectraHostValue,
    chunk_items: Vec<SpectraHostValue>,
}

struct AsyncTcpListenerState {
    listener: mio::net::TcpListener,
    pending_accepts: VecDeque<SpectraHostValue>,
}

struct AsyncTcpStreamState {
    stream: mio::net::TcpStream,
    pending_reads: VecDeque<SpectraHostValue>,
    connect_task: Option<SpectraHostValue>,
    closed: bool,
}

struct AsyncUdpSocketState {
    socket: mio::net::UdpSocket,
    pending_recvs: VecDeque<SpectraHostValue>,
    closed: bool,
}

struct AsyncChannelState {
    queue: VecDeque<SpectraHostValue>,
    capacity: usize,
    pending_sends: VecDeque<(SpectraHostValue, SpectraHostValue)>,
    pending_recvs: VecDeque<SpectraHostValue>,
    closed: bool,
}

enum AsyncStreamPull {
    Pending,
    Item(SpectraHostValue),
    Done,
    Failed,
    Cancelled,
}

trait AsyncHandleKey {
    fn raw(self) -> SpectraHostValue;
}

impl AsyncHandleKey for SpectraHostValue {
    fn raw(self) -> SpectraHostValue {
        self
    }
}

impl AsyncHandleKey for &SpectraHostValue {
    fn raw(self) -> SpectraHostValue {
        *self
    }
}

struct AsyncHandleTable<T> {
    table: HandleTable<T>,
}

impl<T> AsyncHandleTable<T> {
    fn new(kind: HandleKind) -> Self {
        Self {
            table: HandleTable::new(kind),
        }
    }

    fn insert(&mut self, value: T) -> SpectraHostValue {
        self.table.insert(value).raw()
    }

    fn insert_fresh(&mut self, value: T) -> SpectraHostValue {
        self.table.insert_fresh(value).raw()
    }

    fn id<R: AsyncHandleKey>(&self, raw: R) -> Option<HandleId> {
        HandleId::from_raw(raw.raw()).ok()
    }

    fn get<R: AsyncHandleKey>(&self, raw: R) -> Option<&T> {
        self.id(raw).and_then(|id| self.table.get(id).ok())
    }

    fn get_mut<R: AsyncHandleKey>(&mut self, raw: R) -> Option<&mut T> {
        let id = self.id(raw)?;
        self.table.get_mut(id).ok()
    }

    fn remove<R: AsyncHandleKey>(&mut self, raw: R) -> Option<T> {
        let id = self.id(raw)?;
        self.table.remove(id).ok()
    }

    fn contains_key<R: AsyncHandleKey>(&self, raw: R) -> bool {
        self.get(raw).is_some()
    }

    fn clear(&mut self) {
        self.table.clear();
    }

    fn keys(&self) -> impl Iterator<Item = SpectraHostValue> + '_ {
        self.table.iter().map(|(handle, _)| handle.raw())
    }

    fn iter(&self) -> impl Iterator<Item = (SpectraHostValue, &T)> + '_ {
        self.table
            .iter()
            .map(|(handle, value)| (handle.raw(), value))
    }
}
