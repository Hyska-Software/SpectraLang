//! Active run context: a stacking thread-local of run identifiers.
//!
//! R-3213 (plan D5, ADR 0016): enforcement without a reliable notion of the
//! current run is theater, so the runtime needs an explicit run identity before
//! `R-3214` can apply a capability decision at the dispatch point. This module
//! mirrors the proven tracing context stack
//! ([`crate::tracing::CONTEXT_STACK`] and [`crate::tracing::with_context`]): a
//! thread-local stack, a guard type that restores the previous entry on scope
//! exit, explicit propagation for workers, and fail-closed behavior for work
//! that deliberately does not inherit the chain.
//!
//! The repr(C) [`crate::abi::SpectraHostCallContext`] is untouched. The run
//! context is a separate, thread-local concept, so no ABI change is required.
//!
//! Fail-closed detached work: [`spawn_detached`] refuses to start while a run
//! is active unless the spawn carries a grant. Until `R-3214` lands capability
//! evaluation, [`detached_work_allowed`] denies whenever a run is active; a
//! propagation gap therefore surfaces as a visible error instead of a silent
//! host call with no active run.

use std::cell::RefCell;

thread_local! {
    /// Outermost-first chain of runs active on this thread.
    static RUN_STACK: RefCell<Vec<u64>> = const { RefCell::new(Vec::new()) };
}

/// Reported run-context programming errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunContextError {
    /// [`pop`] was called while no run was active.
    EmptyStack,
    /// Detached work was requested while a run is active and the spawn
    /// carried no capability grant (ADR 0016 D5).
    DetachedWorkDenied,
}

impl std::fmt::Display for RunContextError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyStack => write!(formatter, "run context pop on an empty stack"),
            Self::DetachedWorkDenied => write!(
                formatter,
                "detached work refused while a run is active without a capability grant"
            ),
        }
    }
}

impl std::error::Error for RunContextError {}

/// Scope guard returned by [`push`]; pops the run it pushed when dropped.
///
/// A pop on an empty stack is a programming error (an unbalanced pop, or a
/// guard from another thread) and is reported by [`pop`] as
/// [`RunContextError::EmptyStack`]. The guard cannot return an error from
/// `Drop`, so it asserts the invariant in debug builds and leaves the stack
/// empty in release builds.
#[must_use = "the run scope ends as soon as the guard is dropped"]
pub struct RunContextGuard {
    run: u64,
}

impl RunContextGuard {
    /// The run identifier this guard pushed.
    pub fn run(&self) -> u64 {
        self.run
    }

    /// Ends the run scope now instead of at end of scope.
    ///
    /// Consumes the guard so the scope is popped exactly once; the returned
    /// value is the guard's own run, or [`RunContextError::EmptyStack`] if the
    /// stack was already unbalanced.
    pub fn exit(self) -> Result<u64, RunContextError> {
        let popped = pop();
        debug_assert_eq!(
            popped,
            Ok(self.run),
            "run context stack underflow or out-of-order guard exit"
        );
        std::mem::forget(self);
        popped
    }
}

impl Drop for RunContextGuard {
    fn drop(&mut self) {
        let popped = pop();
        debug_assert_eq!(
            popped,
            Ok(self.run),
            "run context stack underflow or out-of-order guard drop"
        );
    }
}

/// Pushes `run` as the innermost active run.
///
/// The returned guard pops it on scope exit. Nested runs stack: the inner run's
/// [`current_chain`] contains the outer runs followed by the inner one, so
/// `R-3214` can intersect capabilities without ever widening authority.
pub fn push(run: u64) -> RunContextGuard {
    RUN_STACK.with(|stack| stack.borrow_mut().push(run));
    RunContextGuard { run }
}

/// Removes and returns the innermost active run.
///
/// An empty stack is a programming error and is reported, never ignored.
pub fn pop() -> Result<u64, RunContextError> {
    RUN_STACK.with(|stack| stack.borrow_mut().pop().ok_or(RunContextError::EmptyStack))
}

/// The innermost active run, if any.
pub fn current() -> Option<u64> {
    RUN_STACK.with(|stack| stack.borrow().last().copied())
}

/// The full active run chain, outermost first.
///
/// A nested run sees its own and its parents' identifiers; `R-3214` intersects
/// the capabilities of every entry so a nested run can only narrow authority.
pub fn current_chain() -> Vec<u64> {
    RUN_STACK.with(|stack| stack.borrow().clone())
}

/// Whether any run is active on this thread.
pub fn is_active() -> bool {
    RUN_STACK.with(|stack| !stack.borrow().is_empty())
}

/// Runs `work` with `run` pushed, restoring the previous run on exit.
pub fn with_run<T>(run: u64, work: impl FnOnce() -> T) -> T {
    let _guard = push(run);
    work()
}

/// Runs `work` with `chain` installed as the entire active chain, restoring the
/// caller's chain when `work` returns or unwinds.
///
/// Workers use this to re-enter the run that spawned them: the worker thread
/// starts with an empty stack, sees exactly the captured chain while the work
/// runs, and is cleared again when the work completes. Installed as a replace
/// (not a push) so a pooled worker can never inherit a stale chain from a
/// previous job.
pub fn with_chain<T>(chain: Vec<u64>, work: impl FnOnce() -> T) -> T {
    struct RestoreGuard(Option<Vec<u64>>);

    impl Drop for RestoreGuard {
        fn drop(&mut self) {
            if let Some(previous) = self.0.take() {
                RUN_STACK.with(|stack| *stack.borrow_mut() = previous);
            }
        }
    }

    let previous = RUN_STACK.with(|stack| std::mem::replace(&mut *stack.borrow_mut(), chain));
    let _restore = RestoreGuard(Some(previous));
    work()
}

/// Fail-closed default for detached work (ADR 0016 D5).
///
/// Work that deliberately does not inherit the run chain must not run while a
/// run is active, because a host call inside it would execute with no active
/// run and no capability decision. Returns `false` while any run is active and
/// `true` otherwise, so programs without a run behave exactly as before.
///
/// `R-3214` replaces this predicate with capability evaluation: a detached
/// spawn carrying an explicit grant will be allowed while its run is active.
pub fn detached_work_allowed() -> bool {
    current().is_none()
}

/// Spawns detached work on a fresh thread.
///
/// Detached work does not inherit the calling thread's run chain: the new
/// thread starts with an empty stack. While a run is active the spawn is
/// refused with [`RunContextError::DetachedWorkDenied`] unless it carries a
/// grant (ADR 0016 D5); `R-3214` fills in the grant evaluation. Outside any run
/// detached work stays allowed.
pub fn spawn_detached<F, T>(work: F) -> Result<std::thread::JoinHandle<T>, RunContextError>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    if !detached_work_allowed() {
        return Err(RunContextError::DetachedWorkDenied);
    }
    Ok(std::thread::spawn(work))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_pop_restores_previous_run() {
        assert_eq!(current(), None);
        let guard = push(11);
        assert_eq!(guard.run(), 11);
        assert_eq!(current(), Some(11));
        assert_eq!(guard.exit(), Ok(11));
        assert_eq!(current(), None);
        assert_eq!(pop(), Err(RunContextError::EmptyStack));
    }

    #[test]
    fn guard_drop_restores_previous_run() {
        let outer = push(1);
        {
            let _inner = push(2);
            assert_eq!(current(), Some(2));
        }
        assert_eq!(current(), Some(1));
        drop(outer);
        assert_eq!(current(), None);
    }

    #[test]
    fn nested_runs_report_their_own_and_parent_chain() {
        let _outer = push(7);
        assert_eq!(current_chain(), vec![7]);
        let inner = push(9);
        assert_eq!(current(), Some(9));
        assert_eq!(current_chain(), vec![7, 9]);
        assert_eq!(inner.exit(), Ok(9));
        assert_eq!(current(), Some(7));
        assert_eq!(current_chain(), vec![7]);
    }

    #[test]
    fn pop_on_empty_stack_is_reported() {
        assert_eq!(current(), None);
        assert_eq!(pop(), Err(RunContextError::EmptyStack));
        // A failed pop leaves the stack usable.
        let guard = push(3);
        assert_eq!(current(), Some(3));
        assert_eq!(guard.exit(), Ok(3));
        assert_eq!(current(), None);
    }

    #[test]
    fn with_run_scopes_the_run_even_on_unwind() {
        let observed = with_run(5, || current());
        assert_eq!(observed, Some(5));
        assert_eq!(current(), None);

        let panicked = std::panic::catch_unwind(|| with_run(6, || panic!("boom")));
        assert!(panicked.is_err());
        assert_eq!(current(), None);
    }

    #[test]
    fn with_chain_replaces_and_restores_the_chain() {
        let _outer = push(1);
        let inside = with_chain(vec![4, 5], current_chain);
        assert_eq!(inside, vec![4, 5]);
        // The caller's own chain is restored, not the captured one.
        assert_eq!(current_chain(), vec![1]);
    }

    #[test]
    fn with_chain_clears_a_stale_worker_chain() {
        // Mirrors a pooled worker: re-entering work with an empty captured
        // chain must not expose the previous job's run.
        let _stale = push(42);
        let observed = with_chain(Vec::new(), current);
        assert_eq!(observed, None);
        assert_eq!(current_chain(), vec![42]);
    }

    #[test]
    fn detached_work_is_refused_inside_a_run() {
        let _guard = push(21);
        assert!(!detached_work_allowed());
        let spawned = spawn_detached(|| 1);
        assert_eq!(spawned.err(), Some(RunContextError::DetachedWorkDenied));
    }

    #[test]
    fn detached_work_runs_and_stays_detached_without_a_run() {
        assert!(detached_work_allowed());
        let (sender, receiver) = std::sync::mpsc::channel();
        let handle = spawn_detached(move || {
            let _ = sender.send(current_chain());
        })
        .expect("detached work outside a run");
        handle.join().expect("detached worker joins");
        assert_eq!(receiver.recv().expect("observation"), Vec::<u64>::new());
        assert_eq!(current(), None);
    }
}
