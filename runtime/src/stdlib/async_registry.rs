use super::*;
pub(crate) struct AsyncTaskRegistry {
    pub(crate) now_ms: SpectraHostValue,
    pub(crate) next_join_order: SpectraHostValue,
    pub(crate) tasks: AsyncHandleTable<AsyncTask>,
    pub(crate) scopes: AsyncHandleTable<AsyncScope>,
    pub(crate) cancel_handles: AsyncHandleTable<SpectraHostValue>,
    pub(crate) streams: AsyncHandleTable<AsyncStream>,
    pub(crate) tcp_listeners: AsyncHandleTable<AsyncTcpListenerState>,
    pub(crate) tcp_streams: AsyncHandleTable<AsyncTcpStreamState>,
    pub(crate) udp_sockets: AsyncHandleTable<AsyncUdpSocketState>,
    pub(crate) async_channels: AsyncHandleTable<AsyncChannelState>,
}

impl AsyncTaskRegistry {
    pub(crate) fn new() -> Self {
        Self {
            now_ms: 0,
            next_join_order: 1,
            tasks: AsyncHandleTable::new(HandleKind::Async),
            scopes: AsyncHandleTable::new(HandleKind::AsyncScope),
            cancel_handles: AsyncHandleTable::new(HandleKind::AsyncCancel),
            streams: AsyncHandleTable::new(HandleKind::AsyncStream),
            tcp_listeners: AsyncHandleTable::new(HandleKind::AsyncTcpListener),
            tcp_streams: AsyncHandleTable::new(HandleKind::AsyncTcpStream),
            udp_sockets: AsyncHandleTable::new(HandleKind::AsyncUdpSocket),
            async_channels: AsyncHandleTable::new(HandleKind::AsyncChannel),
        }
    }

    pub(crate) fn clear(&mut self) {
        self.deregister_io_sources();
        self.now_ms = 0;
        self.next_join_order = 1;
        self.tasks.clear();
        self.scopes.clear();
        self.cancel_handles.clear();
        self.streams.clear();
        self.tcp_listeners.clear();
        self.tcp_streams.clear();
        self.udp_sockets.clear();
        self.async_channels.clear();
    }

    pub(crate) fn deregister_io_sources(&mut self) {
        let listener_ids = self.tcp_listeners.keys().collect::<Vec<_>>();
        for listener_id in listener_ids {
            if let Some(listener) = self.tcp_listeners.get_mut(listener_id) {
                let _ = reactor::global().deregister_source(&mut listener.listener, listener_id);
            }
        }

        let stream_ids = self.tcp_streams.keys().collect::<Vec<_>>();
        for stream_id in stream_ids {
            if let Some(stream) = self.tcp_streams.get_mut(stream_id) {
                let _ = reactor::global().deregister_source(&mut stream.stream, stream_id);
            }
        }

        let socket_ids = self.udp_sockets.keys().collect::<Vec<_>>();
        for socket_id in socket_ids {
            if let Some(socket) = self.udp_sockets.get_mut(socket_id) {
                let _ = reactor::global().deregister_source(&mut socket.socket, socket_id);
            }
        }
    }

    pub(crate) fn allocate_task(
        &mut self,
        value: SpectraHostValue,
        parent_scope: Option<SpectraHostValue>,
        timeout_inner: Option<SpectraHostValue>,
        deadline_ms: Option<SpectraHostValue>,
    ) -> SpectraHostValue {
        self.allocate_task_with_completion(
            value,
            parent_scope,
            timeout_inner,
            deadline_ms,
            true,
            true,
        )
    }

    pub(crate) fn allocate_task_with_completion(
        &mut self,
        value: SpectraHostValue,
        parent_scope: Option<SpectraHostValue>,
        timeout_inner: Option<SpectraHostValue>,
        deadline_ms: Option<SpectraHostValue>,
        completed: bool,
        wake: bool,
    ) -> SpectraHostValue {
        self.allocate_task_with_completion_impl(
            value,
            parent_scope,
            timeout_inner,
            deadline_ms,
            completed,
            wake,
            false,
        )
    }

    pub(crate) fn allocate_task_with_completion_fresh(
        &mut self,
        value: SpectraHostValue,
        parent_scope: Option<SpectraHostValue>,
        timeout_inner: Option<SpectraHostValue>,
        deadline_ms: Option<SpectraHostValue>,
        completed: bool,
        wake: bool,
    ) -> SpectraHostValue {
        self.allocate_task_with_completion_impl(
            value,
            parent_scope,
            timeout_inner,
            deadline_ms,
            completed,
            wake,
            true,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn allocate_task_with_completion_impl(
        &mut self,
        value: SpectraHostValue,
        parent_scope: Option<SpectraHostValue>,
        timeout_inner: Option<SpectraHostValue>,
        deadline_ms: Option<SpectraHostValue>,
        completed: bool,
        wake: bool,
        fresh: bool,
    ) -> SpectraHostValue {
        let task = AsyncTask {
            value,
            cancelled: false,
            failed: false,
            completed,
            parent_scope,
            cancel_handle: 0,
            timeout_inner,
            deadline_ms,
            join_order: None,
        };
        let task_id = if fresh {
            self.tasks.insert_fresh(task)
        } else {
            self.tasks.insert(task)
        };
        let cancel_handle = self.cancel_handles.insert(task_id);
        if let Some(task) = self.tasks.get_mut(task_id) {
            task.cancel_handle = cancel_handle;
        }
        if let Some(scope_id) = parent_scope {
            if let Some(scope) = self.scopes.get_mut(scope_id) {
                scope.children.push(task_id);
            }
        }
        if wake {
            reactor::global().wake_task(task_id);
        }
        task_id
    }

    pub(crate) fn complete_task(&mut self, task_id: SpectraHostValue, value: SpectraHostValue) -> Option<()> {
        let task = self.tasks.get_mut(task_id)?;
        if task.cancelled {
            lock_unpoisoned(background_cancel_hooks()).remove(&task_id);
            notify_async_task_completion();
            return Some(());
        }
        task.value = value;
        task.completed = true;
        lock_unpoisoned(background_cancel_hooks()).remove(&task_id);
        reactor::global().wake_task(task_id);
        notify_async_task_completion();
        Some(())
    }

    pub(crate) fn fail_task(&mut self, task_id: SpectraHostValue) -> Option<()> {
        let task = self.tasks.get_mut(task_id)?;
        task.failed = true;
        task.completed = true;
        lock_unpoisoned(background_cancel_hooks()).remove(&task_id);
        reactor::global().wake_task(task_id);
        notify_async_task_completion();
        Some(())
    }

    pub(crate) fn allocate_failed_task(&mut self) -> SpectraHostValue {
        let task_id = self.allocate_task(-2, None, None, None);
        let _ = self.fail_task(task_id);
        task_id
    }

    pub(crate) fn task_is_cancelled(&self, task_id: SpectraHostValue) -> bool {
        self.tasks
            .get(task_id)
            .map(|task| task.cancelled)
            .unwrap_or(true)
    }

    pub(crate) fn create_scope(&mut self, parent: Option<SpectraHostValue>) -> Option<SpectraHostValue> {
        if let Some(parent_id) = parent {
            self.scopes.get(parent_id)?;
        }
        let scope_id = self.scopes.insert(AsyncScope {
            _parent: parent,
            child_scopes: Vec::new(),
            children: Vec::new(),
            cancelled: false,
            joined_count: 0,
            failures: 0,
        });
        if let Some(parent_id) = parent {
            if let Some(parent_scope) = self.scopes.get_mut(parent_id) {
                parent_scope.child_scopes.push(scope_id);
            }
        }
        Some(scope_id)
    }

    pub(crate) fn attach_task_to_scope(
        &mut self,
        scope_id: SpectraHostValue,
        task_id: SpectraHostValue,
    ) -> Option<()> {
        if !self.scopes.contains_key(scope_id) {
            return None;
        }
        let task = self.tasks.get_mut(task_id)?;
        if let Some(old_scope) = task.parent_scope {
            if let Some(scope) = self.scopes.get_mut(old_scope) {
                scope.children.retain(|child| *child != task_id);
            }
        }
        task.parent_scope = Some(scope_id);
        if let Some(scope) = self.scopes.get_mut(scope_id) {
            if !scope.children.contains(&task_id) {
                scope.children.push(task_id);
            }
        }
        Some(())
    }

    pub(crate) fn cancel_task(&mut self, task_id: SpectraHostValue) -> Option<()> {
        let inner = {
            let task = self.tasks.get_mut(task_id)?;
            task.cancelled = true;
            task.timeout_inner
        };
        if let Some(inner) = inner {
            let _ = self.cancel_task(inner);
        }
        if let Some(cancel) = lock_unpoisoned(background_cancel_hooks()).remove(&task_id) {
            cancel();
        }
        reactor::global().wake_task(task_id);
        notify_async_task_completion();
        Some(())
    }

    pub(crate) fn cancel_scope(&mut self, scope_id: SpectraHostValue) -> Option<()> {
        let (children, child_scopes) = {
            let scope = self.scopes.get_mut(scope_id)?;
            scope.cancelled = true;
            (scope.children.clone(), scope.child_scopes.clone())
        };
        for task_id in children {
            let _ = self.cancel_task(task_id);
        }
        for child_scope in child_scopes {
            let _ = self.cancel_scope(child_scope);
        }
        Some(())
    }

    pub(crate) fn process_due_timeouts(&mut self) {
        let due_tasks: Vec<_> = self
            .tasks
            .iter()
            .filter_map(|(task_id, task)| {
                let deadline = task.deadline_ms?;
                (deadline <= self.now_ms && !task.cancelled).then_some(task_id)
            })
            .collect();
        for task_id in due_tasks {
            let _ = self.cancel_task(task_id);
        }
    }

    pub(crate) fn join_scope(&mut self, scope_id: SpectraHostValue) -> Option<SpectraHostValue> {
        self.process_due_timeouts();
        let (children, child_scopes, scope_cancelled) = {
            let scope = self.scopes.get(scope_id)?;
            (
                scope.children.clone(),
                scope.child_scopes.clone(),
                scope.cancelled,
            )
        };

        let mut joined = 0;
        let mut failures = 0;
        let mut cancelled = scope_cancelled;
        for child_scope in child_scopes {
            let status = self.join_scope(child_scope)?;
            joined += self
                .scopes
                .get(child_scope)
                .map(|scope| scope.joined_count)
                .unwrap_or(0);
            failures += self
                .scopes
                .get(child_scope)
                .map(|scope| scope.failures)
                .unwrap_or(0);
            if status == 1 {
                cancelled = true;
            }
        }

        for task_id in children {
            let Some(task) = self.tasks.get_mut(task_id) else {
                continue;
            };
            if task.join_order.is_none() {
                task.join_order = Some(self.next_join_order);
                self.next_join_order += 1;
            }
            joined += 1;
            if task.failed {
                failures += 1;
            }
            if task.cancelled {
                cancelled = true;
            }
        }

        let scope = self.scopes.get_mut(scope_id)?;
        scope.joined_count = joined;
        scope.failures = failures;
        if failures > 0 {
            Some(2)
        } else if cancelled {
            Some(1)
        } else {
            Some(0)
        }
    }

    pub(crate) fn create_stream(&mut self, kind: AsyncStreamKind, capacity: usize) -> SpectraHostValue {
        
        self.streams.insert(AsyncStream {
            kind,
            buffer: VecDeque::new(),
            capacity,
            pending_next: VecDeque::new(),
            done: false,
            cancelled: false,
            failed: false,
            last_next_status: 0,
            chunk_items: Vec::new(),
        })
    }

    pub(crate) fn push_stream_value(
        &mut self,
        stream_id: SpectraHostValue,
        value: SpectraHostValue,
    ) -> Option<SpectraHostValue> {
        let stream = self.streams.get_mut(stream_id)?;
        if stream.cancelled || stream.done || stream.failed {
            return Some(-1);
        }
        if let Some(task_id) = stream.pending_next.pop_front() {
            stream.last_next_status = 1;
            let _ = self.complete_task(task_id, value);
            self.drive_streams();
            return Some(1);
        }
        if stream.buffer.len() >= stream.capacity {
            return Some(0);
        }
        stream.buffer.push_back(value);
        stream.last_next_status = 1;
        self.drive_streams();
        Some(1)
    }

    pub(crate) fn mark_stream_done(&mut self, stream_id: SpectraHostValue) -> Option<()> {
        let stream = self.streams.get_mut(stream_id)?;
        stream.done = true;
        stream.last_next_status = 2;
        self.drive_streams();
        Some(())
    }

    pub(crate) fn cancel_stream(&mut self, stream_id: SpectraHostValue) -> Option<()> {
        let pending = {
            let stream = self.streams.get_mut(stream_id)?;
            stream.cancelled = true;
            stream.last_next_status = 4;
            stream.pending_next.drain(..).collect::<Vec<_>>()
        };
        for task_id in pending {
            let _ = self.cancel_task(task_id);
        }
        Some(())
    }

    pub(crate) fn drive_streams(&mut self) {
        loop {
            let stream_ids = self.streams.keys().collect::<Vec<_>>();
            let mut progressed = false;
            for stream_id in stream_ids {
                progressed |= self.drive_stream_pending(stream_id);
            }
            if !progressed {
                break;
            }
        }
    }

    pub(crate) fn drive_stream_pending(&mut self, stream_id: SpectraHostValue) -> bool {
        let mut progressed = false;
        while let Some(task_id) = self
                .streams
                .get(stream_id)
                .and_then(|stream| stream.pending_next.front().copied())
        {
            match self.pull_stream_value(stream_id) {
                Some(AsyncStreamPull::Item(value)) => {
                    if let Some(stream) = self.streams.get_mut(stream_id) {
                        stream.pending_next.pop_front();
                        stream.last_next_status = 1;
                    }
                    let _ = self.complete_task(task_id, value);
                    progressed = true;
                }
                Some(AsyncStreamPull::Done) => {
                    if let Some(stream) = self.streams.get_mut(stream_id) {
                        stream.pending_next.pop_front();
                        stream.last_next_status = 2;
                    }
                    let _ = self.complete_task(task_id, -1);
                    progressed = true;
                }
                Some(AsyncStreamPull::Failed) => {
                    if let Some(stream) = self.streams.get_mut(stream_id) {
                        stream.pending_next.pop_front();
                        stream.last_next_status = 3;
                    }
                    if let Some(task) = self.tasks.get_mut(task_id) {
                        task.completed = true;
                        task.failed = true;
                    }
                    progressed = true;
                }
                Some(AsyncStreamPull::Cancelled) => {
                    if let Some(stream) = self.streams.get_mut(stream_id) {
                        stream.pending_next.pop_front();
                        stream.last_next_status = 4;
                    }
                    let _ = self.cancel_task(task_id);
                    progressed = true;
                }
                Some(AsyncStreamPull::Pending) | None => break,
            }
        }
        progressed
    }

    pub(crate) fn pull_stream_value(&mut self, stream_id: SpectraHostValue) -> Option<AsyncStreamPull> {
        let kind = {
            let stream = self.streams.get_mut(stream_id)?;
            if stream.cancelled {
                stream.last_next_status = 4;
                return Some(AsyncStreamPull::Cancelled);
            }
            if stream.failed {
                stream.last_next_status = 3;
                return Some(AsyncStreamPull::Failed);
            }
            if let Some(value) = stream.buffer.pop_front() {
                stream.last_next_status = 1;
                return Some(AsyncStreamPull::Item(value));
            }
            if stream.done {
                stream.last_next_status = 2;
                return Some(AsyncStreamPull::Done);
            }
            stream.kind
        };

        match kind {
            AsyncStreamKind::Source => {
                if let Some(stream) = self.streams.get_mut(stream_id) {
                    stream.last_next_status = 0;
                }
                Some(AsyncStreamPull::Pending)
            }
            AsyncStreamKind::Map { upstream, op, arg } => {
                let pulled = self.pull_stream_value(upstream)?;
                Some(match pulled {
                    AsyncStreamPull::Item(value) => {
                        AsyncStreamPull::Item(map_stream_value(value, op, arg)?)
                    }
                    other => other,
                })
            }
            AsyncStreamKind::Filter {
                upstream,
                predicate,
                arg,
            } => loop {
                let pulled = self.pull_stream_value(upstream)?;
                match pulled {
                    AsyncStreamPull::Item(value) => {
                        if filter_stream_value(value, predicate, arg)? {
                            break Some(AsyncStreamPull::Item(value));
                        }
                    }
                    other => break Some(other),
                }
            },
            AsyncStreamKind::Take {
                upstream,
                remaining,
            } => {
                if remaining <= 0 {
                    if let Some(stream) = self.streams.get_mut(stream_id) {
                        stream.done = true;
                        stream.last_next_status = 2;
                    }
                    return Some(AsyncStreamPull::Done);
                }
                let pulled = self.pull_stream_value(upstream)?;
                if matches!(pulled, AsyncStreamPull::Item(_)) {
                    if let Some(stream) = self.streams.get_mut(stream_id) {
                        if let AsyncStreamKind::Take { remaining, .. } = &mut stream.kind {
                            *remaining -= 1;
                        }
                    }
                }
                Some(pulled)
            }
            AsyncStreamKind::Skip {
                upstream,
                remaining: _,
            } => loop {
                let current_remaining = match self.streams.get(stream_id).map(|stream| stream.kind)
                {
                    Some(AsyncStreamKind::Skip { remaining, .. }) => remaining,
                    _ => return None,
                };
                if current_remaining <= 0 {
                    break self.pull_stream_value(upstream);
                }
                let pulled = self.pull_stream_value(upstream)?;
                match pulled {
                    AsyncStreamPull::Item(_) => {
                        if let Some(stream) = self.streams.get_mut(stream_id) {
                            if let AsyncStreamKind::Skip { remaining, .. } = &mut stream.kind {
                                *remaining -= 1;
                            }
                        }
                    }
                    other => break Some(other),
                }
            },
            AsyncStreamKind::Chunks { upstream, size } => {
                if size <= 0 {
                    return None;
                }
                loop {
                    let current_len = self
                        .streams
                        .get(stream_id)
                        .map(|stream| stream.chunk_items.len())
                        .unwrap_or(0);
                    if current_len >= size as usize {
                        let value = self.take_stream_chunk_sum(stream_id)?;
                        return Some(AsyncStreamPull::Item(value));
                    }
                    let pulled = self.pull_stream_value(upstream)?;
                    match pulled {
                        AsyncStreamPull::Item(value) => {
                            if let Some(stream) = self.streams.get_mut(stream_id) {
                                stream.chunk_items.push(value);
                            }
                        }
                        AsyncStreamPull::Done => {
                            if self
                                .streams
                                .get(stream_id)
                                .map(|stream| !stream.chunk_items.is_empty())
                                .unwrap_or(false)
                            {
                                let value = self.take_stream_chunk_sum(stream_id)?;
                                return Some(AsyncStreamPull::Item(value));
                            }
                            if let Some(stream) = self.streams.get_mut(stream_id) {
                                stream.done = true;
                                stream.last_next_status = 2;
                            }
                            return Some(AsyncStreamPull::Done);
                        }
                        other => return Some(other),
                    }
                }
            }
            AsyncStreamKind::Fuse {
                upstream,
                fused_done,
            } => {
                if fused_done {
                    return Some(AsyncStreamPull::Done);
                }
                let pulled = self.pull_stream_value(upstream)?;
                if matches!(pulled, AsyncStreamPull::Done) {
                    if let Some(stream) = self.streams.get_mut(stream_id) {
                        stream.done = true;
                        if let AsyncStreamKind::Fuse { fused_done, .. } = &mut stream.kind {
                            *fused_done = true;
                        }
                    }
                }
                Some(pulled)
            }
        }
    }

    pub(crate) fn insert_tcp_listener(
        &mut self,
        listener: mio::net::TcpListener,
    ) -> Option<SpectraHostValue> {
        let listener_id = self.tcp_listeners.insert(AsyncTcpListenerState {
            listener,
            pending_accepts: VecDeque::new(),
        });
        let registered = self
            .tcp_listeners
            .get_mut(listener_id)
            .map(|state| {
                reactor::global().register_source(
                    &mut state.listener,
                    listener_id,
                    Interest::READABLE,
                )
            })
            .unwrap_or(false);
        if !registered {
            let _ = self.tcp_listeners.remove(listener_id);
            return None;
        }
        Some(listener_id)
    }

    pub(crate) fn insert_tcp_stream(
        &mut self,
        stream: mio::net::TcpStream,
        connect_task: Option<SpectraHostValue>,
    ) -> Option<SpectraHostValue> {
        let stream_id = self.tcp_streams.insert(AsyncTcpStreamState {
            stream,
            pending_reads: VecDeque::new(),
            connect_task,
            closed: false,
        });
        let registered = self
            .tcp_streams
            .get_mut(stream_id)
            .map(|state| {
                reactor::global().register_source(
                    &mut state.stream,
                    stream_id,
                    Interest::READ_WRITE,
                )
            })
            .unwrap_or(false);
        if !registered {
            let _ = self.tcp_streams.remove(stream_id);
            return None;
        }
        Some(stream_id)
    }

    pub(crate) fn insert_udp_socket(&mut self, socket: mio::net::UdpSocket) -> Option<SpectraHostValue> {
        let socket_id = self.udp_sockets.insert(AsyncUdpSocketState {
            socket,
            pending_recvs: VecDeque::new(),
            closed: false,
        });
        let registered = self
            .udp_sockets
            .get_mut(socket_id)
            .map(|state| {
                reactor::global().register_source(
                    &mut state.socket,
                    socket_id,
                    Interest::READABLE,
                )
            })
            .unwrap_or(false);
        if !registered {
            let _ = self.udp_sockets.remove(socket_id);
            return None;
        }
        Some(socket_id)
    }

    pub(crate) fn drive_tcp_connect(&mut self, ready_token: Option<SpectraHostValue>) {
        let stream_ids = self.tcp_streams.keys().collect::<Vec<_>>();
        for stream_id in stream_ids {
            let Some(connect_task) = self
                .tcp_streams
                .get(stream_id)
                .and_then(|stream| stream.connect_task)
            else {
                continue;
            };

            if self.task_is_cancelled(connect_task) {
                if let Some(stream) = self.tcp_streams.get_mut(stream_id) {
                    let _ = reactor::global().deregister_source(&mut stream.stream, stream_id);
                }
                let _ = self.tcp_streams.remove(stream_id);
                continue;
            }

            if ready_token != Some(stream_id) {
                continue;
            }

            let result = self
                .tcp_streams
                .get_mut(stream_id)
                .map(|stream| stream.stream.take_error());
            match result {
                Some(Ok(None)) => {
                    if let Some(stream) = self.tcp_streams.get_mut(stream_id) {
                        stream.connect_task = None;
                    }
                    let _ = self.complete_task(connect_task, stream_id);
                }
                Some(Ok(Some(_))) | Some(Err(_)) | None => {
                    if let Some(stream) = self.tcp_streams.get_mut(stream_id) {
                        let _ = reactor::global().deregister_source(&mut stream.stream, stream_id);
                    }
                    let _ = self.tcp_streams.remove(stream_id);
                    let _ = self.fail_task(connect_task);
                }
            }
        }
    }

    pub(crate) fn drive_tcp_accepts(&mut self) {
        let listener_ids = self.tcp_listeners.keys().collect::<Vec<_>>();
        for listener_id in listener_ids {
            loop {
                let task_id = match self
                    .tcp_listeners
                    .get_mut(listener_id)
                    .and_then(|listener| listener.pending_accepts.pop_front())
                {
                    Some(task_id) if self.task_is_cancelled(task_id) => continue,
                    Some(task_id) => task_id,
                    None => break,
                };

                let accepted = match self.tcp_listeners.get(listener_id) {
                    Some(listener) => listener.listener.accept(),
                    None => break,
                };
                match accepted {
                    Ok((stream, _)) => {
                        let Some(stream_id) = self.insert_tcp_stream(stream, None) else {
                            let _ = self.fail_task(task_id);
                            continue;
                        };
                        let _ = self.complete_task(task_id, stream_id);
                    }
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                        if let Some(listener) = self.tcp_listeners.get_mut(listener_id) {
                            listener.pending_accepts.push_front(task_id);
                        }
                        break;
                    }
                    Err(_) => {
                        let _ = self.fail_task(task_id);
                    }
                }
            }
        }
    }

    pub(crate) fn drive_tcp_reads(&mut self) {
        let stream_ids = self.tcp_streams.keys().collect::<Vec<_>>();
        for stream_id in stream_ids {
            loop {
                let task_id = match self
                    .tcp_streams
                    .get_mut(stream_id)
                    .and_then(|stream| stream.pending_reads.pop_front())
                {
                    Some(task_id) if self.task_is_cancelled(task_id) => continue,
                    Some(task_id) => task_id,
                    None => break,
                };

                let mut byte = [0u8; 1];
                let read = match self.tcp_streams.get_mut(stream_id) {
                    Some(state) if state.closed => Ok(0),
                    Some(state) => state.stream.read(&mut byte),
                    None => break,
                };
                match read {
                    Ok(0) => {
                        let _ = self.complete_task(task_id, -1);
                    }
                    Ok(_) => {
                        let _ = self.complete_task(task_id, byte[0] as SpectraHostValue);
                    }
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                        if let Some(state) = self.tcp_streams.get_mut(stream_id) {
                            state.pending_reads.push_front(task_id);
                        }
                        break;
                    }
                    Err(_) => {
                        let _ = self.fail_task(task_id);
                    }
                }
            }
        }
    }

    pub(crate) fn drive_udp_recvs(&mut self) {
        let socket_ids = self.udp_sockets.keys().collect::<Vec<_>>();
        for socket_id in socket_ids {
            loop {
                let task_id = match self
                    .udp_sockets
                    .get_mut(socket_id)
                    .and_then(|socket| socket.pending_recvs.pop_front())
                {
                    Some(task_id) if self.task_is_cancelled(task_id) => continue,
                    Some(task_id) => task_id,
                    None => break,
                };

                let mut byte = [0u8; 1];
                let recv = match self.udp_sockets.get_mut(socket_id) {
                    Some(state) if state.closed => match "127.0.0.1:0".parse() {
                        Ok(addr) => Ok((0, addr)),
                        Err(_) => {
                            let _ = self.fail_task(task_id);
                            continue;
                        }
                    },
                    Some(state) => state.socket.recv_from(&mut byte),
                    None => break,
                };
                match recv {
                    Ok((0, _)) => {
                        let _ = self.complete_task(task_id, -1);
                    }
                    Ok((_, _)) => {
                        let _ = self.complete_task(task_id, byte[0] as SpectraHostValue);
                    }
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                        if let Some(state) = self.udp_sockets.get_mut(socket_id) {
                            state.pending_recvs.push_front(task_id);
                        }
                        break;
                    }
                    Err(_) => {
                        let _ = self.fail_task(task_id);
                    }
                }
            }
        }
    }

    pub(crate) fn drive_pending_io_for_task(&mut self, task_id: SpectraHostValue) {
        for _ in 0..16 {
            let pending = self
                .tasks
                .get(task_id)
                .map(|task| !task.completed && !task.cancelled && !task.failed)
                .unwrap_or(false);
            if !pending {
                break;
            }
            while let Some(event) = reactor::global().poll(Some(Duration::ZERO)) {
                self.process_reactor_event(event);
            }
            self.drive_tcp_connect(None);
            self.drive_tcp_accepts();
            self.drive_tcp_reads();
            self.drive_udp_recvs();
            std::thread::yield_now();
        }
    }

    pub(crate) fn process_reactor_event(&mut self, event: ReactorEvent) {
        match event.kind {
            reactor::EventKind::Io => {
                self.drive_tcp_connect(Some(event.token));
                self.drive_tcp_accepts();
                self.drive_tcp_reads();
                self.drive_udp_recvs();
            }
            reactor::EventKind::Timer => self.process_due_timeouts(),
            reactor::EventKind::TaskWake => {}
        }
    }

    pub(crate) fn drive_async_channel(&mut self, channel_id: SpectraHostValue) -> Option<()> {
        loop {
            let recv_task = {
                let channel = self.async_channels.get_mut(channel_id)?;
                channel.pending_recvs.pop_front()
            };
            let Some(recv_task) = recv_task else {
                break;
            };
            if self.task_is_cancelled(recv_task) {
                continue;
            }

            let delivered = {
                let channel = self.async_channels.get_mut(channel_id)?;
                channel.queue.pop_front()
            };
            if let Some(value) = delivered {
                let _ = self.complete_task(recv_task, value);
                continue;
            }

            let pending_send = {
                let channel = self.async_channels.get_mut(channel_id)?;
                channel.pending_sends.pop_front()
            };
            if let Some((send_task, value)) = pending_send {
                if !self.task_is_cancelled(send_task) {
                    let _ = self.complete_task(send_task, 1);
                    let _ = self.complete_task(recv_task, value);
                    continue;
                }
            }

            let closed = self
                .async_channels
                .get(channel_id)
                .map(|channel| channel.closed)
                .unwrap_or(true);
            if closed {
                let _ = self.complete_task(recv_task, -1);
                continue;
            }

            if let Some(channel) = self.async_channels.get_mut(channel_id) {
                channel.pending_recvs.push_front(recv_task);
            }
            break;
        }

        loop {
            let can_buffer = self
                .async_channels
                .get(channel_id)
                .map(|channel| channel.queue.len() < channel.capacity)
                .unwrap_or(false);
            if !can_buffer {
                break;
            }
            let pending_send = {
                let channel = self.async_channels.get_mut(channel_id)?;
                channel.pending_sends.pop_front()
            };
            let Some((send_task, value)) = pending_send else {
                break;
            };
            if self.task_is_cancelled(send_task) {
                continue;
            }
            if let Some(channel) = self.async_channels.get_mut(channel_id) {
                channel.queue.push_back(value);
            }
            let _ = self.complete_task(send_task, 1);
        }
        Some(())
    }
}
