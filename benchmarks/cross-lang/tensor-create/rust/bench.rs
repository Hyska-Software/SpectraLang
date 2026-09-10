// Phase 31: tensor-create (Rust)

fn main() {
    let iters: usize = 60;
    let mut total: usize = 0;
    for _ in 0..iters {
        let t: Vec<f64> = vec![1.0_f64; 1_048_576];
        total += t.len();
        drop(t);
    }
    if total != 62_914_560 {
        eprintln!("unexpected: {}", total);
        std::process::exit(1);
    }
}
