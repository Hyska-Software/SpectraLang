impl Trace for String {
    fn trace(&self, _ctx: &mut dyn TraceContext) {}
}

impl<T> Trace for Option<T>
where
    T: Trace,
{
    fn trace(&self, ctx: &mut dyn TraceContext) {
        if let Some(inner) = self {
            inner.trace(ctx);
        }
    }
}

impl<T, E> Trace for Result<T, E>
where
    T: Trace,
    E: Trace,
{
    fn trace(&self, ctx: &mut dyn TraceContext) {
        match self {
            Ok(ok) => ok.trace(ctx),
            Err(err) => err.trace(ctx),
        }
    }
}

impl<T> Trace for Box<T>
where
    T: Trace + ?Sized,
{
    fn trace(&self, ctx: &mut dyn TraceContext) {
        (**self).trace(ctx);
    }
}

impl<T> Trace for Vec<T>
where
    T: Trace,
{
    fn trace(&self, ctx: &mut dyn TraceContext) {
        for item in self {
            item.trace(ctx);
        }
    }
}

impl<T, const N: usize> Trace for [T; N]
where
    T: Trace,
{
    fn trace(&self, ctx: &mut dyn TraceContext) {
        for item in self {
            item.trace(ctx);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct Node {
        _value: i32,
        next: Option<Gc<Node>>,
    }

    impl Trace for Node {
        fn trace(&self, ctx: &mut dyn TraceContext) {
            self.next.trace(ctx);
        }
    }

    #[test]
    fn manual_allocation_tracks_statistics() {
        let _lock = crate::runtime_test_guard();
        let config = MemoryConfig {
            manual_soft_limit_bytes: 64,
            ..MemoryConfig::default()
        };
        let memory = HybridMemory::with_config(config);

        {
            let first = memory
                .allocate_manual([0_u8; 16])
                .expect("first allocation");
            assert_eq!(memory.stats().manual.bytes, 16);
            let second = memory
                .allocate_manual([0_u8; 32])
                .expect("second allocation");
            assert_eq!(memory.stats().manual.allocations, 2);
            drop(first);
            assert_eq!(memory.stats().manual.allocations, 1);
            drop(second);
        }

        assert_eq!(memory.stats().manual.bytes, 0);
    }

    #[test]
    fn manual_allocation_respects_soft_limit() {
        let _lock = crate::runtime_test_guard();
        let config = MemoryConfig {
            manual_soft_limit_bytes: 32,
            ..MemoryConfig::default()
        };
        let memory = HybridMemory::with_config(config);

        let _first = memory.allocate_manual([0_u8; 16]).expect("within limit");
        assert!(memory
            .allocate_manual([0_u8; 24])
            .unwrap_err()
            .manual_limit_exceeded()
            .is_some());
    }

    #[test]
    fn unreachable_traced_objects_are_collected() {
        let _lock = crate::runtime_test_guard();
        let memory = HybridMemory::default();
        let node = memory.allocate_traced(Node::default());
        assert!(node.is_alive());
        memory.collect_garbage();
        assert!(!node.is_alive());
    }

    #[test]
    fn rooted_objects_survive_collection() {
        let _lock = crate::runtime_test_guard();
        let memory = HybridMemory::default();
        let node = memory.allocate_traced(Node::default());
        let root = node.into_root();

        memory.collect_garbage();
        assert!(node.is_alive());

        drop(root);
        memory.collect_garbage();
        assert!(!node.is_alive());
    }

    #[test]
    fn traced_children_keep_each_other_alive() {
        let _lock = crate::runtime_test_guard();
        let memory = HybridMemory::default();
        let parent = memory.allocate_traced(Node::default());
        let child = memory.allocate_traced(Node {
            _value: 1,
            next: None,
        });
        {
            let mut borrow = parent.borrow_mut();
            borrow.next = Some(child.clone());
        }
        let root = parent.into_root();
        memory.collect_garbage();
        assert!(child.is_alive());

        drop(root);
        memory.collect_garbage();
        assert!(!parent.is_alive());
        assert!(!child.is_alive());
    }
}
