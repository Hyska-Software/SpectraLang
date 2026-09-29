# ADR 0020: Collection Semantics and Performance Contracts

Status: Accepted for implementation; release baselines are tracked by R-3301.

Date: 2026-09-27

Roadmap item: R-3301

## Context

`std.collections` exposes fixed-size arrays plus handle-backed `List<T>`,
`Vector<T>`, `Map<K,V>`, insertion-ordered `Set<T>`, `Stack<T>`, and
`Queue<T>`. Their runtime representations are `VecDeque<SpectraHostValue>`,
`Vec<SpectraHostValue>`, `HashMap<CollectionKey, SpectraHostValue>`,
`Vec<SpectraHostValue>`, `Vec<SpectraHostValue>`, and
`VecDeque<SpectraHostValue>` respectively.
Every operation crosses a typed host-call boundary and validates a generational
handle. The runtime must preserve that ownership and validation model while
adding structures whose algorithms compare values internally.

The key constraint is that an ABI word is not a comparison contract. String
addresses, boxed aggregate addresses, and float bit patterns cannot be treated
as interchangeable with source-level values. In particular, using a
Spectra callback inside a hash probe, tree rotation, heap sift, or union-find
loop would add unpredictable dispatch and reentrancy to each operation.

## Decisions

### D1 — Keep existing collection behavior

- `array<T>` remains contiguous and fixed-size. `List<T>` remains the
  growable, double-ended indexed collection backed by `VecDeque`.
- `Vector<T>` is a separate growable contiguous sequence backed by `Vec`;
  append/pop and indexed access are O(1) amortized/constant time, while middle
  insertion/removal shift elements in O(n).
- `Map<K,V>` remains hash-based. Its key snapshot stays deterministic and
  ordered by value; its current string-key normalization copies UTF-8 content
  into an owned `String` for hashing/equality. Iteration remains a snapshot.
- `Set<T>` remains insertion-ordered and value-based. `Stack<T>` remains LIFO
  with snapshots in storage order (bottom to top); `Queue<T>` remains FIFO
  with snapshots from front to back.
- A separate `Deque<T>` is not added in this phase: the current `List<T>`
  already supports indexed and double-ended operations, while `Queue<T>` is
  the FIFO contract.

### D2 — Define value identity for hash and tree keys

Hashable and orderable collection keys are limited to source primitive values
whose ABI representation is defined by the compiler: booleans, characters,
integers (including exact-width integers), floats, and strings. Aggregate,
function, opaque-handle, and unresolved values are rejected during semantic
analysis; their raw ABI words are never used as object identity.

- Strings use UTF-8 content identity and ordering, independent of allocation
  address. The current runtime-owned normalization allocation is part of the
  measured string-key cost; no pointer-hash shortcut is allowed.
- Integer, boolean, character, and exact-width integer operations use their
  source type's value semantics. Internal comparison receives a compiler
  type tag so signedness and width are explicit.
- Floating ordering uses IEEE `total_cmp` semantics. Hash-set floating
  identity is the same total-order equivalence class, so equal keys always
  hash equally, signed zero is distinct, and NaN payloads have deterministic
  identity. This intentionally differs from arithmetic `==` for NaN.
- A typed collection fixes its key/element type on first insertion when the
  constructor has no source-level type argument. Each operation validates its
  hidden compiler-supplied type tag against that stored kind.

No collection algorithm invokes a Spectra comparator callback. Type-directed
Rust comparison/hash code runs inside the runtime operation.

### D3 — Add complementary structures, not aliases

- `HashSet<T>` is unordered and hash-backed, for expected constant-time
  membership; `Set<T>` keeps insertion order.
- `OrderedMap<K,V>` is tree-backed, iterates by key, and supports key ranges;
  `Map<K,V>` keeps expected constant-time hash lookup and its deterministic
  snapshot contract.
- `PriorityQueue<T>` is a contiguous binary heap. It is max-first by default,
  supports an explicit min-first constructor, and does not promise stable order
  among equal-priority values.
- `BitSet` stores dense non-negative indexes as packed `u64` words.
- `DisjointSet` stores parent and size arrays and uses path halving plus union
  by size.
- `HashSet<T>`, `PriorityQueue<T>` and `BitSet` expose O(1) actual-capacity
  queries; `BitSet` reports addressable bit positions rounded to complete
  `u64` words.

### D4 — Capacity and failure behavior

Capacity arguments are signed language integers. Negative values are invalid;
conversion and `len + additional` arithmetic are checked. Runtime containers
use fallible reservation (`try_reserve` / `try_reserve_exact` where appropriate)
and translate capacity overflow or allocator refusal to the normal host-call
runtime error path. New handles are not published until construction succeeds.
The runtime does not turn a failed reservation into a Rust panic.

The existing collections, `Vector<T>`, `HashSet<T>`, `PriorityQueue<T>` and
`BitSet` expose O(1) `*_capacity` queries so callers and benchmarks can observe
actual reserved storage without allocator instrumentation.

Values stored beyond their calling frame are escaped using the runtime's
existing stored-value ownership hook. Handle tables retain generational
validation, and stale handles fail with the established not-found status.
Iteration materializes owned snapshots while holding the collection lock, then
releases all registry/collection locks before Spectra code consumes the
iterator.

### D5 — Qualify `Vector<T>` with a storage benchmark

The R-3301 release storage probe showed at least 10% repeatable speedup over
`VecDeque` at medium and large sizes in all three independent groups for
append-and-traverse, with equal backing bytes. The small-size sample did not
meet the speed threshold. This qualifies `Vector<T>` for implementation under
R-3309. R-3301 also records fixed-array and Spectra `List<T>` JIT/AOT results
for context. The isolated storage probe does not claim Spectra ABI wall-clock
performance; the implemented API remains subject to the R-3308 JIT/AOT gate.

## Consequences

The contract catalog, semantic checks, generated lowering tables, runtime
registries and JIT/AOT host-call bindings must be updated together. Benchmark
reports separate algorithmic operation costs from host-call overhead and
string normalization. A fast ABI or locking change requires repeatable release
evidence of at least 10% on its named workload and unchanged correctness/AOT
behavior. New structures stay beta until the R-3308 gate passes. R-3309 adds
`Vector<T>` only after the isolated storage benchmark qualifies it.

## Evidence status

The checked-in R-3301 report records the `Vector<T>` storage candidate as
qualified at medium and large sizes. It is prototype evidence only; public
runtime performance remains pending until the R-3308 release gate completes.
