// Phase 31: cpu-string-build (Rust)

fn main() {
    let iters: i64 = 500;
    let mut total: usize = 0;
    for _ in 0..iters {
        let mut s = String::with_capacity(200);
        for _ in 0..100 {
            s.push_str("x|");
        }
        total += s.len();
    }
    if total != 100_000 {
        eprintln!("unexpected: {}", total);
        std::process::exit(1);
    }
}
