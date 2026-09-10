// Phase 31: async-echo (Rust)

use std::sync::{Arc, Mutex};
use std::thread;

fn main() {
    let iters: usize = 10000;
    let mut total: i64 = 0;
    for _ in 0..iters {
        let local = Arc::new(Mutex::new(0_i64));
        let mut handles = Vec::with_capacity(10);
        for k in 1..=10 {
            let local = Arc::clone(&local);
            handles.push(thread::spawn(move || {
                let mut g = local.lock().unwrap();
                *g += k as i64;
            }));
        }
        for h in handles {
            h.join().unwrap();
        }
        let g = local.lock().unwrap();
        total += *g;
    }
    if total != 550_000 {
        eprintln!("unexpected: {}", total);
        std::process::exit(1);
    }
}
