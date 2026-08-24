//! Platform-selected async reactor used by the Phase 21 scheduler boundary.
//!
//! The reactor is deliberately below Spectra language semantics: it only moves
//! readiness events. `Task<T>`, cancellation propagation, timeouts as language
//! helpers, and structured scopes are layered above it by later roadmap items.
//!
//! Platform notes:
//!
//! - Linux selects the `epoll` backend label.
//! - Windows selects the `IOCP` backend label.
//! - macOS and the BSD family select the `kqueue` backend label.
//! - unsupported targets select a portable fallback backend so tests and tools
//!   can still exercise the same interface.
//!
//! The public interface is intentionally shared across task wakeups, timer
//! readiness, and I/O readiness. The implementation keeps the scheduler event
//! queue in process and wakes a real `mio::Poll` multiplexer underneath.
//! `mio::Poll` maps to the platform readiness backend (`epoll`, `IOCP`, or
//! `kqueue`) where the target supports it.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

use mio::{Events, Interest as MioInterest, Poll, Token, Waker};

const REACTOR_WAKE_TOKEN: Token = Token(0);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendKind {
    LinuxEpoll,
    WindowsIocp,
    MacosKqueue,
    Fallback,
}

impl BackendKind {
    pub fn as_name(self) -> &'static str {
        match self {
            Self::LinuxEpoll => "epoll",
            Self::WindowsIocp => "iocp",
            Self::MacosKqueue => "kqueue",
            Self::Fallback => "fallback",
        }
    }

    pub fn as_code(self) -> i64 {
        match self {
            Self::LinuxEpoll => 1,
            Self::WindowsIocp => 2,
            Self::MacosKqueue => 3,
            Self::Fallback => 0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventKind {
    TaskWake,
    Timer,
    Io,
}

impl EventKind {
    pub fn as_code(self) -> i64 {
        match self {
            Self::TaskWake => 1,
            Self::Timer => 2,
            Self::Io => 3,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Interest(u8);

impl Interest {
    pub const READABLE: Self = Self(0b01);
    pub const WRITABLE: Self = Self(0b10);
    pub const READ_WRITE: Self = Self(0b11);

    pub fn from_bits(bits: i64) -> Option<Self> {
        let bits = (bits & 0b11) as u8;
        (bits != 0).then_some(Self(bits))
    }

    pub fn bits(self) -> i64 {
        self.0 as i64
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReactorEvent {
    pub kind: EventKind,
    pub token: i64,
    pub readiness: Interest,
}

impl ReactorEvent {
    fn task(token: i64) -> Self {
        Self {
            kind: EventKind::TaskWake,
            token,
            readiness: Interest::READ_WRITE,
        }
    }

    fn timer(token: i64) -> Self {
        Self {
            kind: EventKind::Timer,
            token,
            readiness: Interest::READ_WRITE,
        }
    }

    fn io(token: i64, readiness: Interest) -> Self {
        Self {
            kind: EventKind::Io,
            token,
            readiness,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ReactorStats {
    pub queued: usize,
    pub task_wakeups: u64,
    pub timer_events: u64,
    pub io_events: u64,
    pub io_registrations: usize,
}

#[derive(Debug, Clone, Copy)]
struct IoRegistration {
    interest: Interest,
}

#[derive(Debug, Default)]
struct ReactorState {
    queue: VecDeque<ReactorEvent>,
    io: HashMap<i64, IoRegistration>,
    task_wakeups: u64,
    timer_events: u64,
    io_events: u64,
    generation: u64,
}

#[derive(Debug)]
struct ReactorCore {
    state: Mutex<ReactorState>,
    ready: Condvar,
    os: Option<OsMultiplexer>,
}

#[derive(Debug)]
struct OsMultiplexer {
    poll: Mutex<Poll>,
    events: Mutex<Events>,
    waker: Waker,
}

impl ReactorCore {
    fn new() -> Self {
        Self {
            state: Mutex::new(ReactorState::default()),
            ready: Condvar::new(),
            os: OsMultiplexer::new(),
        }
    }

    fn push_event(&self, event: ReactorEvent) {
        if let Ok(mut state) = self.state.lock() {
            match event.kind {
                EventKind::TaskWake => state.task_wakeups += 1,
                EventKind::Timer => state.timer_events += 1,
                EventKind::Io => state.io_events += 1,
            }
            state.queue.push_back(event);
            self.ready.notify_one();
        }
        if let Some(os) = &self.os {
            let _ = os.waker.wake();
        }
    }

    fn push_event_for_generation(&self, generation: u64, event: ReactorEvent) {
        if let Ok(mut state) = self.state.lock() {
            if state.generation != generation {
                return;
            }
            match event.kind {
                EventKind::TaskWake => state.task_wakeups += 1,
                EventKind::Timer => state.timer_events += 1,
                EventKind::Io => state.io_events += 1,
            }
            state.queue.push_back(event);
            self.ready.notify_one();
        }
        if let Some(os) = &self.os {
            let _ = os.waker.wake();
        }
    }

    fn generation(&self) -> u64 {
        self.state.lock().map(|state| state.generation).unwrap_or(0)
    }

    fn pop_event(&self, timeout: Option<Duration>) -> Option<ReactorEvent> {
        let mut state = self.state.lock().ok()?;
        if let Some(event) = state.queue.pop_front() {
            return Some(event);
        }

        match timeout {
            Some(duration) if duration.is_zero() => None,
            Some(duration) if self.os.is_some() => {
                drop(state);
                let deadline = Instant::now() + duration;
                loop {
                    let now = Instant::now();
                    if now >= deadline {
                        return None;
                    }
                    self.poll_os(Some(deadline - now));
                    if let Some(event) = self.state.lock().ok()?.queue.pop_front() {
                        return Some(event);
                    }
                }
            }
            Some(duration) => {
                let (mut state, _) = self.ready.wait_timeout(state, duration).ok()?;
                state.queue.pop_front()
            }
            None if self.os.is_some() => {
                drop(state);
                loop {
                    self.poll_os(None);
                    if let Some(event) = self.state.lock().ok()?.queue.pop_front() {
                        return Some(event);
                    }
                }
            }
            None => loop {
                state = self.ready.wait(state).ok()?;
                if let Some(event) = state.queue.pop_front() {
                    return Some(event);
                }
            },
        }
    }

    fn poll_os(&self, timeout: Option<Duration>) {
        let Some(os) = &self.os else {
            return;
        };
        let Ok(mut poll) = os.poll.lock() else {
            return;
        };
        let Ok(mut events) = os.events.lock() else {
            return;
        };
        events.clear();
        let _ = poll.poll(&mut events, timeout);
        let readiness = events
            .iter()
            .filter_map(|event| {
                if event.token() == REACTOR_WAKE_TOKEN {
                    return None;
                }

                let mut bits = 0;
                if event.is_readable() || event.is_read_closed() || event.is_error() {
                    bits |= Interest::READABLE.bits();
                }
                if event.is_writable() || event.is_write_closed() || event.is_error() {
                    bits |= Interest::WRITABLE.bits();
                }
                Interest::from_bits(bits).map(|interest| (event.token().0 as i64, interest))
            })
            .collect::<Vec<_>>();
        drop(events);
        drop(poll);

        for (token, interest) in readiness {
            self.push_io_event(token, interest);
        }
    }

    fn push_io_event(&self, token: i64, readiness: Interest) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        let Some(registration) = state.io.get(&token) else {
            return;
        };
        let Some(readiness) = Interest::from_bits(registration.interest.bits() & readiness.bits())
        else {
            return;
        };
        state.io_events += 1;
        state.queue.push_back(ReactorEvent::io(token, readiness));
        self.ready.notify_one();
    }
}

impl OsMultiplexer {
    fn new() -> Option<Self> {
        let poll = Poll::new().ok()?;
        let waker = Waker::new(poll.registry(), REACTOR_WAKE_TOKEN).ok()?;
        Some(Self {
            poll: Mutex::new(poll),
            events: Mutex::new(Events::with_capacity(1024)),
            waker,
        })
    }
}

#[derive(Debug, Clone)]
pub struct Reactor {
    backend: BackendKind,
    core: Arc<ReactorCore>,
}

impl Reactor {
    pub fn new() -> Self {
        Self {
            backend: selected_backend(),
            core: Arc::new(ReactorCore::new()),
        }
    }

    pub fn backend(&self) -> BackendKind {
        self.backend
    }

    pub fn wake_task(&self, task: i64) {
        self.core.push_event(ReactorEvent::task(task));
    }

    pub fn register_timer(&self, token: i64, delay: Duration) {
        let core = Arc::clone(&self.core);
        let generation = core.generation();
        thread::spawn(move || {
            let deadline = Instant::now() + delay;
            let now = Instant::now();
            if deadline > now {
                thread::sleep(deadline - now);
            }
            core.push_event_for_generation(generation, ReactorEvent::timer(token));
        });
    }

    pub fn register_io(&self, token: i64, interest: Interest) -> bool {
        let Ok(mut state) = self.core.state.lock() else {
            return false;
        };
        state.io.insert(token, IoRegistration { interest });
        true
    }

    /// Register a real mio source with the platform multiplexer.  The
    /// synthetic `register_io`/`notify_io` pair remains available for host
    /// adapters, but production socket readiness must use this path so the
    /// selected epoll/IOCP/kqueue backend owns the event.
    pub fn register_source<S: mio::event::Source + ?Sized>(
        &self,
        source: &mut S,
        token: i64,
        interest: Interest,
    ) -> bool {
        if token < 0 {
            return false;
        }
        let Some(mio_interest) = mio_interest(interest) else {
            return false;
        };
        let Some(os) = &self.core.os else {
            return false;
        };
        let Ok(poll) = os.poll.lock() else {
            return false;
        };
        if poll
            .registry()
            .register(source, Token(token as usize), mio_interest)
            .is_err()
        {
            return false;
        }
        drop(poll);

        let Ok(mut state) = self.core.state.lock() else {
            return false;
        };
        state.io.insert(token, IoRegistration { interest });
        true
    }

    /// Remove a source from the platform multiplexer and from the reactor's
    /// token registry.
    pub fn deregister_source<S: mio::event::Source + ?Sized>(
        &self,
        source: &mut S,
        token: i64,
    ) -> bool {
        let Some(os) = &self.core.os else {
            return false;
        };
        let Ok(poll) = os.poll.lock() else {
            return false;
        };
        let deregistered = poll.registry().deregister(source).is_ok();
        drop(poll);
        if let Ok(mut state) = self.core.state.lock() {
            state.io.remove(&token);
        }
        deregistered
    }

    pub fn notify_io(&self, token: i64, readiness: Interest) -> bool {
        let interest = {
            let Ok(state) = self.core.state.lock() else {
                return false;
            };
            let Some(registration) = state.io.get(&token) else {
                return false;
            };
            registration.interest
        };

        let ready_bits = interest.bits() & readiness.bits();
        if ready_bits == 0 {
            return false;
        }
        let Some(readiness) = Interest::from_bits(ready_bits) else {
            return false;
        };
        self.core.push_event(ReactorEvent::io(token, readiness));
        true
    }

    pub fn poll(&self, timeout: Option<Duration>) -> Option<ReactorEvent> {
        self.core.pop_event(timeout)
    }

    pub fn drain(&self, limit: usize) -> Vec<ReactorEvent> {
        let mut events = Vec::new();
        for _ in 0..limit {
            let Some(event) = self.poll(Some(Duration::ZERO)) else {
                break;
            };
            events.push(event);
        }
        events
    }

    pub fn stats(&self) -> ReactorStats {
        let Ok(state) = self.core.state.lock() else {
            return ReactorStats::default();
        };
        ReactorStats {
            queued: state.queue.len(),
            task_wakeups: state.task_wakeups,
            timer_events: state.timer_events,
            io_events: state.io_events,
            io_registrations: state.io.len(),
        }
    }

    pub fn reset(&self) {
        if let Ok(mut state) = self.core.state.lock() {
            let generation = state.generation.wrapping_add(1);
            *state = ReactorState::default();
            state.generation = generation;
            self.core.ready.notify_all();
        }
    }
}

impl Default for Reactor {
    fn default() -> Self {
        Self::new()
    }
}

pub fn global() -> &'static Reactor {
    static REACTOR: OnceLock<Reactor> = OnceLock::new();
    REACTOR.get_or_init(Reactor::new)
}

#[cfg(target_os = "linux")]
fn selected_backend() -> BackendKind {
    BackendKind::LinuxEpoll
}

#[cfg(target_os = "windows")]
fn selected_backend() -> BackendKind {
    BackendKind::WindowsIocp
}

#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "freebsd",
    target_os = "openbsd",
    target_os = "netbsd",
    target_os = "dragonfly"
))]
fn selected_backend() -> BackendKind {
    BackendKind::MacosKqueue
}

#[cfg(not(any(
    target_os = "linux",
    target_os = "windows",
    target_os = "macos",
    target_os = "ios",
    target_os = "freebsd",
    target_os = "openbsd",
    target_os = "netbsd",
    target_os = "dragonfly"
)))]
fn selected_backend() -> BackendKind {
    BackendKind::Fallback
}

fn mio_interest(interest: Interest) -> Option<MioInterest> {
    match interest.bits() {
        1 => Some(MioInterest::READABLE),
        2 => Some(MioInterest::WRITABLE),
        3 => Some(MioInterest::READABLE.add(MioInterest::WRITABLE)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selects_platform_backend() {
        let backend = Reactor::new().backend();
        #[cfg(target_os = "linux")]
        assert_eq!(backend, BackendKind::LinuxEpoll);
        #[cfg(target_os = "windows")]
        assert_eq!(backend, BackendKind::WindowsIocp);
        #[cfg(any(
            target_os = "macos",
            target_os = "ios",
            target_os = "freebsd",
            target_os = "openbsd",
            target_os = "netbsd",
            target_os = "dragonfly"
        ))]
        assert_eq!(backend, BackendKind::MacosKqueue);
        #[cfg(not(any(
            target_os = "linux",
            target_os = "windows",
            target_os = "macos",
            target_os = "ios",
            target_os = "freebsd",
            target_os = "openbsd",
            target_os = "netbsd",
            target_os = "dragonfly"
        )))]
        assert_eq!(backend, BackendKind::Fallback);
    }

    #[cfg(any(
        target_os = "freebsd",
        target_os = "openbsd",
        target_os = "netbsd",
        target_os = "dragonfly"
    ))]
    #[test]
    fn bsd_kqueue_backend_is_selected() {
        assert_eq!(Reactor::new().backend(), BackendKind::MacosKqueue);
    }

    #[test]
    fn task_timer_and_io_events_share_one_queue() {
        let reactor = Reactor::new();
        reactor.wake_task(10);
        assert!(reactor.register_io(20, Interest::READABLE));
        assert!(reactor.notify_io(20, Interest::READABLE));
        reactor.register_timer(30, Duration::from_millis(1));

        let mut kinds = Vec::new();
        for _ in 0..3 {
            if let Some(event) = reactor.poll(Some(Duration::from_millis(100))) {
                kinds.push(event.kind);
            }
        }

        assert!(kinds.contains(&EventKind::TaskWake));
        assert!(kinds.contains(&EventKind::Io));
        assert!(kinds.contains(&EventKind::Timer));
    }

    #[test]
    fn real_tcp_listener_readiness_reaches_the_shared_queue() {
        let reactor = Reactor::new();
        let mut listener = mio::net::TcpListener::bind(
            "127.0.0.1:0".parse().expect("loopback address must parse"),
        )
        .expect("loopback listener must bind");
        let address = listener.local_addr().expect("listener must expose address");
        let token = 41;

        assert!(reactor.register_source(&mut listener, token, Interest::READABLE));
        let _client = std::net::TcpStream::connect(address).expect("loopback connect");

        let event = reactor
            .poll(Some(Duration::from_secs(2)))
            .expect("mio must report listener readiness");
        assert_eq!(event.kind, EventKind::Io);
        assert_eq!(event.token, token);
        assert_eq!(event.readiness, Interest::READABLE);

        let (_accepted, _) = listener.accept().expect("ready listener must accept");
        assert!(reactor.deregister_source(&mut listener, token));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_epoll_backend_handles_10k_suspended_task_wakeups() {
        let reactor = Reactor::new();
        assert_eq!(reactor.backend(), BackendKind::LinuxEpoll);

        for task in 0..10_000 {
            reactor.wake_task(task);
        }

        let mut seen = 0usize;
        while reactor.poll(Some(Duration::ZERO)).is_some() {
            seen += 1;
        }

        assert_eq!(seen, 10_000);
        assert_eq!(reactor.stats().task_wakeups, 10_000);
    }
}
