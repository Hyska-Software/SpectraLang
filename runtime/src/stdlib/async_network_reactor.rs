use super::*;
pub(crate) extern "C" fn std_async_fs_read(ctx: *mut SpectraHostCallContext) -> i32 {
    let (args, results) = match host_call_args(ctx, 1) {
        Ok(parts) => parts,
        Err(status) => return status,
    };
    let path = unsafe {
        match read_fs_path_arg(args[0]) {
            Ok(Some(path)) => path,
            Ok(None) => return HOST_STATUS_INVALID_ARGUMENT,
            Err(status) => return status,
        }
    };
    let task = match spawn_cancellable_io_task(
        move || match std::fs::read_to_string(&path) {
            Ok(content) => {
                let ptr = unsafe { alloc_spectra_string(&content) };
                if ptr == 0 {
                    Err(())
                } else {
                    Ok(ptr)
                }
            }
            Err(_) => Err(()),
        },
        || {},
    ) {
        Ok(task) => task,
        Err(status) => return status,
    };
    results[0] = task;
    HOST_STATUS_SUCCESS
}

pub(crate) extern "C" fn std_async_fs_write(ctx: *mut SpectraHostCallContext) -> i32 {
    let (args, results) = match host_call_args(ctx, 2) {
        Ok(parts) => parts,
        Err(status) => return status,
    };
    let path = unsafe {
        match read_fs_path_arg(args[0]) {
            Ok(Some(path)) => path,
            Ok(None) => return HOST_STATUS_INVALID_ARGUMENT,
            Err(status) => return status,
        }
    };
    let content = unsafe {
        match read_spectra_string(args[1]) {
            Some(content) => content,
            None => return HOST_STATUS_INVALID_ARGUMENT,
        }
    };
    let byte_len = content.len() as SpectraHostValue;
    let task = match spawn_cancellable_io_task(
        move || {
            if fs_write_text(&path, &content, false) {
                Ok(byte_len)
            } else {
                Err(())
            }
        },
        || {},
    ) {
        Ok(task) => task,
        Err(status) => return status,
    };
    results[0] = task;
    HOST_STATUS_SUCCESS
}

pub(crate) extern "C" fn std_async_tcp_listen(ctx: *mut SpectraHostCallContext) -> i32 {
    let (args, results) = match host_call_args(ctx, 1) {
        Ok(parts) => parts,
        Err(status) => return status,
    };
    if !(0..=65_535).contains(&args[0]) {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    let address = std::net::SocketAddr::from(([127, 0, 0, 1], args[0] as u16));
    let listener = match mio::net::TcpListener::bind(address) {
        Ok(listener) => listener,
        Err(_) => return HOST_STATUS_INTERNAL_ERROR,
    };
    let mut registry = match lock_async_task_registry() {
        Ok(registry) => registry,
        Err(status) => return status,
    };
    let Some(listener_id) = registry.insert_tcp_listener(listener) else {
        return HOST_STATUS_INTERNAL_ERROR;
    };
    results[0] = listener_id;
    HOST_STATUS_SUCCESS
}

pub(crate) extern "C" fn std_async_tcp_listener_port(ctx: *mut SpectraHostCallContext) -> i32 {
    let (args, results) = match host_call_args(ctx, 1) {
        Ok(parts) => parts,
        Err(status) => return status,
    };
    let registry = match lock_async_task_registry() {
        Ok(registry) => registry,
        Err(status) => return status,
    };
    let Some(listener) = registry.tcp_listeners.get(args[0]) else {
        return HOST_STATUS_NOT_FOUND;
    };
    let Ok(addr) = listener.listener.local_addr() else {
        return HOST_STATUS_INTERNAL_ERROR;
    };
    results[0] = addr.port() as SpectraHostValue;
    HOST_STATUS_SUCCESS
}

pub(crate) extern "C" fn std_async_tcp_connect(ctx: *mut SpectraHostCallContext) -> i32 {
    let (args, results) = match host_call_args(ctx, 1) {
        Ok(parts) => parts,
        Err(status) => return status,
    };
    if !(1..=65_535).contains(&args[0]) {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    let address = std::net::SocketAddr::from(([127, 0, 0, 1], args[0] as u16));
    match mio::net::TcpStream::connect(address) {
        Ok(stream) => {
            let mut registry = match lock_async_task_registry() {
                Ok(registry) => registry,
                Err(status) => return status,
            };
            let connect_task =
                registry.allocate_task_with_completion(0, None, None, None, false, true);
            if registry
                .insert_tcp_stream(stream, Some(connect_task))
                .is_none()
            {
                let _ = registry.fail_task(connect_task);
            }
            results[0] = connect_task;
        }
        Err(_) => {
            let mut registry = match lock_async_task_registry() {
                Ok(registry) => registry,
                Err(status) => return status,
            };
            results[0] = registry.allocate_failed_task();
        }
    }
    HOST_STATUS_SUCCESS
}

pub(crate) extern "C" fn std_async_tcp_accept(ctx: *mut SpectraHostCallContext) -> i32 {
    let (args, results) = match host_call_args(ctx, 1) {
        Ok(parts) => parts,
        Err(status) => return status,
    };
    let mut registry = match lock_async_task_registry() {
        Ok(registry) => registry,
        Err(status) => return status,
    };
    let Some(listener) = registry.tcp_listeners.get(args[0]) else {
        return HOST_STATUS_NOT_FOUND;
    };
    match listener.listener.accept() {
        Ok((stream, _)) => {
            let Some(stream_id) = registry.insert_tcp_stream(stream, None) else {
                results[0] = registry.allocate_failed_task();
                return HOST_STATUS_SUCCESS;
            };
            results[0] = registry.allocate_task(stream_id, None, None, None);
        }
        Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
            let task_id = registry.allocate_task_with_completion(0, None, None, None, false, true);
            if let Some(listener) = registry.tcp_listeners.get_mut(args[0]) {
                listener.pending_accepts.push_back(task_id);
            }
            results[0] = task_id;
        }
        Err(_) => {
            results[0] = registry.allocate_failed_task();
        }
    }
    HOST_STATUS_SUCCESS
}

pub(crate) extern "C" fn std_async_tcp_read(ctx: *mut SpectraHostCallContext) -> i32 {
    let (args, results) = match host_call_args(ctx, 1) {
        Ok(parts) => parts,
        Err(status) => return status,
    };
    let mut registry = match lock_async_task_registry() {
        Ok(registry) => registry,
        Err(status) => return status,
    };
    let mut byte = [0u8; 1];
    let read = match registry.tcp_streams.get_mut(args[0]) {
        Some(state) if state.closed => Ok(0),
        Some(state) => state.stream.read(&mut byte),
        None => return HOST_STATUS_NOT_FOUND,
    };
    match read {
        Ok(0) => results[0] = registry.allocate_task(-1, None, None, None),
        Ok(_) => results[0] = registry.allocate_task(byte[0] as SpectraHostValue, None, None, None),
        Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
            let task_id = registry.allocate_task_with_completion(0, None, None, None, false, true);
            if let Some(state) = registry.tcp_streams.get_mut(args[0]) {
                state.pending_reads.push_back(task_id);
            }
            results[0] = task_id;
        }
        Err(_) => results[0] = registry.allocate_failed_task(),
    }
    HOST_STATUS_SUCCESS
}

pub(crate) extern "C" fn std_async_tcp_write(ctx: *mut SpectraHostCallContext) -> i32 {
    let (args, results) = match host_call_args(ctx, 2) {
        Ok(parts) => parts,
        Err(status) => return status,
    };
    if !(0..=255).contains(&args[1]) {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    let mut registry = match lock_async_task_registry() {
        Ok(registry) => registry,
        Err(status) => return status,
    };
    let write = match registry.tcp_streams.get_mut(args[0]) {
        Some(state) if state.closed => Ok(0),
        Some(state) => state.stream.write(&[args[1] as u8]),
        None => return HOST_STATUS_NOT_FOUND,
    };
    match write {
        Ok(count) => {
            results[0] = registry.allocate_task(count as SpectraHostValue, None, None, None)
        }
        Err(_) => results[0] = registry.allocate_failed_task(),
    }
    registry.drive_tcp_reads();
    HOST_STATUS_SUCCESS
}

pub(crate) extern "C" fn std_async_tcp_close(ctx: *mut SpectraHostCallContext) -> i32 {
    let (args, results) = match host_call_args(ctx, 1) {
        Ok(parts) => parts,
        Err(status) => return status,
    };
    let mut registry = match lock_async_task_registry() {
        Ok(registry) => registry,
        Err(status) => return status,
    };
    if let Some(state) = registry.tcp_streams.get_mut(args[0]) {
        let _ = reactor::global().deregister_source(&mut state.stream, args[0]);
        state.closed = true;
        results[0] = 1;
        registry.drive_tcp_reads();
        return HOST_STATUS_SUCCESS;
    }
    if let Some(listener) = registry.tcp_listeners.get_mut(args[0]) {
        let _ = reactor::global().deregister_source(&mut listener.listener, args[0]);
        let _ = registry.tcp_listeners.remove(args[0]);
        results[0] = 1;
        return HOST_STATUS_SUCCESS;
    }
    HOST_STATUS_NOT_FOUND
}

pub(crate) extern "C" fn std_async_udp_bind(ctx: *mut SpectraHostCallContext) -> i32 {
    let (args, results) = match host_call_args(ctx, 1) {
        Ok(parts) => parts,
        Err(status) => return status,
    };
    if !(0..=65_535).contains(&args[0]) {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    let address = std::net::SocketAddr::from(([127, 0, 0, 1], args[0] as u16));
    let socket = match mio::net::UdpSocket::bind(address) {
        Ok(socket) => socket,
        Err(_) => return HOST_STATUS_INTERNAL_ERROR,
    };
    let mut registry = match lock_async_task_registry() {
        Ok(registry) => registry,
        Err(status) => return status,
    };
    let Some(socket_id) = registry.insert_udp_socket(socket) else {
        return HOST_STATUS_INTERNAL_ERROR;
    };
    results[0] = socket_id;
    HOST_STATUS_SUCCESS
}

pub(crate) extern "C" fn std_async_udp_port(ctx: *mut SpectraHostCallContext) -> i32 {
    let (args, results) = match host_call_args(ctx, 1) {
        Ok(parts) => parts,
        Err(status) => return status,
    };
    let registry = match lock_async_task_registry() {
        Ok(registry) => registry,
        Err(status) => return status,
    };
    let Some(socket) = registry.udp_sockets.get(args[0]) else {
        return HOST_STATUS_NOT_FOUND;
    };
    let Ok(addr) = socket.socket.local_addr() else {
        return HOST_STATUS_INTERNAL_ERROR;
    };
    results[0] = addr.port() as SpectraHostValue;
    HOST_STATUS_SUCCESS
}

pub(crate) extern "C" fn std_async_udp_send_to(ctx: *mut SpectraHostCallContext) -> i32 {
    let (args, results) = match host_call_args(ctx, 3) {
        Ok(parts) => parts,
        Err(status) => return status,
    };
    if !(1..=65_535).contains(&args[1]) || !(0..=255).contains(&args[2]) {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    let mut registry = match lock_async_task_registry() {
        Ok(registry) => registry,
        Err(status) => return status,
    };
    let send = match registry.udp_sockets.get(args[0]) {
        Some(state) if state.closed => Ok(0),
        Some(state) => state.socket.send_to(
            &[args[2] as u8],
            std::net::SocketAddr::from(([127, 0, 0, 1], args[1] as u16)),
        ),
        None => return HOST_STATUS_NOT_FOUND,
    };
    match send {
        Ok(count) => {
            results[0] = registry.allocate_task(count as SpectraHostValue, None, None, None)
        }
        Err(_) => results[0] = registry.allocate_failed_task(),
    }
    registry.drive_udp_recvs();
    HOST_STATUS_SUCCESS
}

pub(crate) extern "C" fn std_async_udp_recv(ctx: *mut SpectraHostCallContext) -> i32 {
    let (args, results) = match host_call_args(ctx, 1) {
        Ok(parts) => parts,
        Err(status) => return status,
    };
    let mut registry = match lock_async_task_registry() {
        Ok(registry) => registry,
        Err(status) => return status,
    };
    let mut byte = [0u8; 1];
    let recv = match registry.udp_sockets.get_mut(args[0]) {
        Some(state) if state.closed => match "127.0.0.1:0".parse() {
            Ok(addr) => Ok((0, addr)),
            Err(_) => return HOST_STATUS_INTERNAL_ERROR,
        },
        Some(state) => state.socket.recv_from(&mut byte),
        None => return HOST_STATUS_NOT_FOUND,
    };
    match recv {
        Ok((0, _)) => results[0] = registry.allocate_task(-1, None, None, None),
        Ok((_, _)) => {
            results[0] = registry.allocate_task(byte[0] as SpectraHostValue, None, None, None)
        }
        Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
            let task_id = registry.allocate_task_with_completion(0, None, None, None, false, true);
            if let Some(state) = registry.udp_sockets.get_mut(args[0]) {
                state.pending_recvs.push_back(task_id);
            }
            results[0] = task_id;
        }
        Err(_) => results[0] = registry.allocate_failed_task(),
    }
    HOST_STATUS_SUCCESS
}

pub(crate) extern "C" fn std_async_udp_close(ctx: *mut SpectraHostCallContext) -> i32 {
    let (args, results) = match host_call_args(ctx, 1) {
        Ok(parts) => parts,
        Err(status) => return status,
    };
    let mut registry = match lock_async_task_registry() {
        Ok(registry) => registry,
        Err(status) => return status,
    };
    let Some(state) = registry.udp_sockets.get_mut(args[0]) else {
        return HOST_STATUS_NOT_FOUND;
    };
    let _ = reactor::global().deregister_source(&mut state.socket, args[0]);
    state.closed = true;
    results[0] = 1;
    registry.drive_udp_recvs();
    HOST_STATUS_SUCCESS
}

pub(crate) extern "C" fn std_async_channel_new(ctx: *mut SpectraHostCallContext) -> i32 {
    let (args, results) = match host_call_args(ctx, 1) {
        Ok(parts) => parts,
        Err(status) => return status,
    };
    if args[0] <= 0 {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    let mut registry = match lock_async_task_registry() {
        Ok(registry) => registry,
        Err(status) => return status,
    };
    let channel_id = registry.async_channels.insert(AsyncChannelState {
        queue: VecDeque::new(),
        capacity: args[0] as usize,
        pending_sends: VecDeque::new(),
        pending_recvs: VecDeque::new(),
        closed: false,
    });
    results[0] = channel_id;
    HOST_STATUS_SUCCESS
}

pub(crate) extern "C" fn std_async_channel_send(ctx: *mut SpectraHostCallContext) -> i32 {
    let (args, results) = match host_call_args(ctx, 2) {
        Ok(parts) => parts,
        Err(status) => return status,
    };
    let mut registry = match lock_async_task_registry() {
        Ok(registry) => registry,
        Err(status) => return status,
    };
    let Some(channel) = registry.async_channels.get_mut(args[0]) else {
        return HOST_STATUS_NOT_FOUND;
    };
    if channel.closed {
        results[0] = registry.allocate_task(0, None, None, None);
        return HOST_STATUS_SUCCESS;
    }
    if let Some(recv_task) = channel.pending_recvs.pop_front() {
        if !registry.task_is_cancelled(recv_task) {
            let _ = registry.complete_task(recv_task, args[1]);
            results[0] = registry.allocate_task(1, None, None, None);
            return HOST_STATUS_SUCCESS;
        }
    }
    let Some(channel) = registry.async_channels.get_mut(args[0]) else {
        return HOST_STATUS_NOT_FOUND;
    };
    if channel.queue.len() < channel.capacity {
        channel.queue.push_back(args[1]);
        results[0] = registry.allocate_task(1, None, None, None);
    } else {
        let task_id = registry.allocate_task_with_completion(0, None, None, None, false, true);
        if let Some(channel) = registry.async_channels.get_mut(args[0]) {
            channel.pending_sends.push_back((task_id, args[1]));
        }
        results[0] = task_id;
    }
    HOST_STATUS_SUCCESS
}

pub(crate) extern "C" fn std_async_channel_recv(ctx: *mut SpectraHostCallContext) -> i32 {
    let (args, results) = match host_call_args(ctx, 1) {
        Ok(parts) => parts,
        Err(status) => return status,
    };
    let mut registry = match lock_async_task_registry() {
        Ok(registry) => registry,
        Err(status) => return status,
    };
    if !registry.async_channels.contains_key(args[0]) {
        return HOST_STATUS_NOT_FOUND;
    }
    let queued = registry
        .async_channels
        .get_mut(args[0])
        .and_then(|channel| channel.queue.pop_front());
    if let Some(value) = queued {
        results[0] = registry.allocate_task(value, None, None, None);
        let _ = registry.drive_async_channel(args[0]);
        return HOST_STATUS_SUCCESS;
    }
    let pending_send = registry
        .async_channels
        .get_mut(args[0])
        .and_then(|channel| channel.pending_sends.pop_front());
    if let Some((send_task, value)) = pending_send {
        if !registry.task_is_cancelled(send_task) {
            let _ = registry.complete_task(send_task, 1);
            results[0] = registry.allocate_task(value, None, None, None);
            return HOST_STATUS_SUCCESS;
        }
    }
    let closed = registry
        .async_channels
        .get(args[0])
        .map(|channel| channel.closed)
        .unwrap_or(true);
    if closed {
        results[0] = registry.allocate_task(-1, None, None, None);
        return HOST_STATUS_SUCCESS;
    }
    let task_id = registry.allocate_task_with_completion(0, None, None, None, false, true);
    if let Some(channel) = registry.async_channels.get_mut(args[0]) {
        channel.pending_recvs.push_back(task_id);
    }
    results[0] = task_id;
    HOST_STATUS_SUCCESS
}

pub(crate) extern "C" fn std_async_channel_close(ctx: *mut SpectraHostCallContext) -> i32 {
    let (args, results) = match host_call_args(ctx, 1) {
        Ok(parts) => parts,
        Err(status) => return status,
    };
    let mut registry = match lock_async_task_registry() {
        Ok(registry) => registry,
        Err(status) => return status,
    };
    let Some(channel) = registry.async_channels.get_mut(args[0]) else {
        return HOST_STATUS_NOT_FOUND;
    };
    channel.closed = true;
    results[0] = 1;
    let _ = registry.drive_async_channel(args[0]);
    HOST_STATUS_SUCCESS
}

pub(crate) extern "C" fn std_async_channel_len(ctx: *mut SpectraHostCallContext) -> i32 {
    let (args, results) = match host_call_args(ctx, 1) {
        Ok(parts) => parts,
        Err(status) => return status,
    };
    let registry = match lock_async_task_registry() {
        Ok(registry) => registry,
        Err(status) => return status,
    };
    let Some(channel) = registry.async_channels.get(args[0]) else {
        return HOST_STATUS_NOT_FOUND;
    };
    results[0] = channel.queue.len() as SpectraHostValue;
    HOST_STATUS_SUCCESS
}

pub(crate) extern "C" fn std_async_task_reset(ctx: *mut SpectraHostCallContext) -> i32 {
    let args = match host_call_void_args(ctx, 0) {
        Ok(args) => args,
        Err(status) => return status,
    };
    let _ = args;
    let cancellation_hooks = {
        let _lifecycle = lock_unpoisoned(background_task_lifecycle());
        let mut registry = match lock_async_task_registry() {
            Ok(registry) => registry,
            Err(status) => return status,
        };
        let cancellation_hooks = {
            let mut hooks = lock_unpoisoned(background_cancel_hooks());
            hooks.drain().map(|(_, hook)| hook).collect::<Vec<_>>()
        };
        registry.clear();
        cancellation_hooks
    };
    for cancel in cancellation_hooks {
        cancel();
    }
    reactor::global().reset();
    if set_async_last_reactor_event(None).is_err() {
        return HOST_STATUS_INTERNAL_ERROR;
    }
    HOST_STATUS_SUCCESS
}

pub(crate) extern "C" fn std_async_reactor_backend(ctx: *mut SpectraHostCallContext) -> i32 {
    let (_, results) = match host_call_args(ctx, 0) {
        Ok(parts) => parts,
        Err(status) => return status,
    };
    results[0] = reactor::global().backend().as_code();
    HOST_STATUS_SUCCESS
}

pub(crate) extern "C" fn std_async_reactor_wake(ctx: *mut SpectraHostCallContext) -> i32 {
    let (args, results) = match host_call_args(ctx, 1) {
        Ok(parts) => parts,
        Err(status) => return status,
    };
    reactor::global().wake_task(args[0]);
    results[0] = 1;
    HOST_STATUS_SUCCESS
}

pub(crate) extern "C" fn std_async_reactor_timer(ctx: *mut SpectraHostCallContext) -> i32 {
    let (args, results) = match host_call_args(ctx, 2) {
        Ok(parts) => parts,
        Err(status) => return status,
    };
    if args[1] < 0 {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    reactor::global().register_timer(args[0], Duration::from_millis(args[1] as u64));
    results[0] = 1;
    HOST_STATUS_SUCCESS
}

pub(crate) extern "C" fn std_async_reactor_io_register(ctx: *mut SpectraHostCallContext) -> i32 {
    let (args, results) = match host_call_args(ctx, 2) {
        Ok(parts) => parts,
        Err(status) => return status,
    };
    let Some(interest) = Interest::from_bits(args[1]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    results[0] = i64::from(reactor::global().register_io(args[0], interest));
    HOST_STATUS_SUCCESS
}

pub(crate) extern "C" fn std_async_reactor_io_notify(ctx: *mut SpectraHostCallContext) -> i32 {
    let (args, results) = match host_call_args(ctx, 2) {
        Ok(parts) => parts,
        Err(status) => return status,
    };
    let Some(readiness) = Interest::from_bits(args[1]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    results[0] = i64::from(reactor::global().notify_io(args[0], readiness));
    HOST_STATUS_SUCCESS
}

pub(crate) extern "C" fn std_async_reactor_poll(ctx: *mut SpectraHostCallContext) -> i32 {
    let (args, results) = match host_call_args(ctx, 1) {
        Ok(parts) => parts,
        Err(status) => return status,
    };
    if args[0] < -1 {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    let timeout = if args[0] < 0 {
        None
    } else {
        Some(Duration::from_millis(args[0] as u64))
    };
    let event = reactor::global().poll(timeout);
    if set_async_last_reactor_event(event).is_err() {
        return HOST_STATUS_INTERNAL_ERROR;
    }
    results[0] = event.map(|event| event.token).unwrap_or(-1);
    HOST_STATUS_SUCCESS
}

pub(crate) extern "C" fn std_async_reactor_last_kind(ctx: *mut SpectraHostCallContext) -> i32 {
    let (_, results) = match host_call_args(ctx, 0) {
        Ok(parts) => parts,
        Err(status) => return status,
    };
    let last = match async_last_reactor_event().lock() {
        Ok(last) => last,
        Err(_) => return HOST_STATUS_INTERNAL_ERROR,
    };
    results[0] = last.map(|event| event.kind.as_code()).unwrap_or(0);
    HOST_STATUS_SUCCESS
}

pub(crate) extern "C" fn std_async_reactor_last_readiness(ctx: *mut SpectraHostCallContext) -> i32 {
    let (_, results) = match host_call_args(ctx, 0) {
        Ok(parts) => parts,
        Err(status) => return status,
    };
    let last = match async_last_reactor_event().lock() {
        Ok(last) => last,
        Err(_) => return HOST_STATUS_INTERNAL_ERROR,
    };
    results[0] = last.map(|event| event.readiness.bits()).unwrap_or(0);
    HOST_STATUS_SUCCESS
}

pub(crate) extern "C" fn std_async_reactor_stats_queued(ctx: *mut SpectraHostCallContext) -> i32 {
    let (_, results) = match host_call_args(ctx, 0) {
        Ok(parts) => parts,
        Err(status) => return status,
    };
    results[0] = reactor::global().stats().queued as i64;
    HOST_STATUS_SUCCESS
}

pub(crate) extern "C" fn std_async_reactor_stats_task_wakeups(ctx: *mut SpectraHostCallContext) -> i32 {
    let (_, results) = match host_call_args(ctx, 0) {
        Ok(parts) => parts,
        Err(status) => return status,
    };
    results[0] = reactor::global().stats().task_wakeups as i64;
    HOST_STATUS_SUCCESS
}

pub(crate) extern "C" fn std_async_reactor_stats_timer_events(ctx: *mut SpectraHostCallContext) -> i32 {
    let (_, results) = match host_call_args(ctx, 0) {
        Ok(parts) => parts,
        Err(status) => return status,
    };
    results[0] = reactor::global().stats().timer_events as i64;
    HOST_STATUS_SUCCESS
}

pub(crate) extern "C" fn std_async_reactor_stats_io_events(ctx: *mut SpectraHostCallContext) -> i32 {
    let (_, results) = match host_call_args(ctx, 0) {
        Ok(parts) => parts,
        Err(status) => return status,
    };
    results[0] = reactor::global().stats().io_events as i64;
    HOST_STATUS_SUCCESS
}

pub(crate) extern "C" fn std_async_reactor_stats_io_registrations(ctx: *mut SpectraHostCallContext) -> i32 {
    let (_, results) = match host_call_args(ctx, 0) {
        Ok(parts) => parts,
        Err(status) => return status,
    };
    results[0] = reactor::global().stats().io_registrations as i64;
    HOST_STATUS_SUCCESS
}
