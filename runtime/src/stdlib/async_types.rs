use super::*;
pub(crate) fn host_call_args<'a>(
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

pub(crate) fn host_call_void_args<'a>(
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
pub(crate) struct AsyncTask {
    pub(crate) value: SpectraHostValue,
    pub(crate) cancelled: bool,
    pub(crate) failed: bool,
    pub(crate) completed: bool,
    pub(crate) parent_scope: Option<SpectraHostValue>,
    pub(crate) cancel_handle: SpectraHostValue,
    pub(crate) timeout_inner: Option<SpectraHostValue>,
    pub(crate) deadline_ms: Option<SpectraHostValue>,
    pub(crate) join_order: Option<SpectraHostValue>,
}

pub(crate) struct AsyncScope {
    pub(crate) _parent: Option<SpectraHostValue>,
    pub(crate) child_scopes: Vec<SpectraHostValue>,
    pub(crate) children: Vec<SpectraHostValue>,
    pub(crate) cancelled: bool,
    pub(crate) joined_count: SpectraHostValue,
    pub(crate) failures: SpectraHostValue,
}

#[derive(Clone, Copy)]
pub(crate) enum AsyncStreamKind {
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

pub(crate) struct AsyncStream {
    pub(crate) kind: AsyncStreamKind,
    pub(crate) buffer: VecDeque<SpectraHostValue>,
    pub(crate) capacity: usize,
    pub(crate) pending_next: VecDeque<SpectraHostValue>,
    pub(crate) done: bool,
    pub(crate) cancelled: bool,
    pub(crate) failed: bool,
    pub(crate) last_next_status: SpectraHostValue,
    pub(crate) chunk_items: Vec<SpectraHostValue>,
}

pub(crate) struct AsyncTcpListenerState {
    pub(crate) listener: mio::net::TcpListener,
    pub(crate) pending_accepts: VecDeque<SpectraHostValue>,
}

pub(crate) struct AsyncTcpStreamState {
    pub(crate) stream: mio::net::TcpStream,
    pub(crate) pending_reads: VecDeque<SpectraHostValue>,
    pub(crate) connect_task: Option<SpectraHostValue>,
    pub(crate) closed: bool,
}

pub(crate) struct AsyncUdpSocketState {
    pub(crate) socket: mio::net::UdpSocket,
    pub(crate) pending_recvs: VecDeque<SpectraHostValue>,
    pub(crate) closed: bool,
}

pub(crate) struct AsyncChannelState {
    pub(crate) queue: VecDeque<SpectraHostValue>,
    pub(crate) capacity: usize,
    pub(crate) pending_sends: VecDeque<(SpectraHostValue, SpectraHostValue)>,
    pub(crate) pending_recvs: VecDeque<SpectraHostValue>,
    pub(crate) closed: bool,
}

pub(crate) enum AsyncStreamPull {
    Pending,
    Item(SpectraHostValue),
    Done,
    Failed,
    Cancelled,
}

pub(crate) trait AsyncHandleKey {
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

pub(crate) struct AsyncHandleTable<T> {
    table: HandleTable<T>,
}

impl<T> AsyncHandleTable<T> {
    pub(crate) fn new(kind: HandleKind) -> Self {
        Self {
            table: HandleTable::new(kind),
        }
    }

    pub(crate) fn insert(&mut self, value: T) -> SpectraHostValue {
        self.table.insert(value).raw()
    }

    pub(crate) fn insert_fresh(&mut self, value: T) -> SpectraHostValue {
        self.table.insert_fresh(value).raw()
    }

    pub(crate) fn id<R: AsyncHandleKey>(&self, raw: R) -> Option<HandleId> {
        HandleId::from_raw(raw.raw()).ok()
    }

    pub(crate) fn get<R: AsyncHandleKey>(&self, raw: R) -> Option<&T> {
        self.id(raw).and_then(|id| self.table.get(id).ok())
    }

    pub(crate) fn get_mut<R: AsyncHandleKey>(&mut self, raw: R) -> Option<&mut T> {
        let id = self.id(raw)?;
        self.table.get_mut(id).ok()
    }

    pub(crate) fn remove<R: AsyncHandleKey>(&mut self, raw: R) -> Option<T> {
        let id = self.id(raw)?;
        self.table.remove(id).ok()
    }

    pub(crate) fn contains_key<R: AsyncHandleKey>(&self, raw: R) -> bool {
        self.get(raw).is_some()
    }

    pub(crate) fn clear(&mut self) {
        self.table.clear();
    }

    pub(crate) fn keys(&self) -> impl Iterator<Item = SpectraHostValue> + '_ {
        self.table.iter().map(|(handle, _)| handle.raw())
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = (SpectraHostValue, &T)> + '_ {
        self.table
            .iter()
            .map(|(handle, value)| (handle.raw(), value))
    }
}
