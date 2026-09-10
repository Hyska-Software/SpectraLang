// Phase 31: cpu-hashmap (Rust)

use std::collections::HashMap;

fn main() {
    let n: i64 = 200;
    let iters: i64 = 300;
    let mut acc: i64 = 0;
    for _ in 0..iters {
        let mut m: HashMap<i64, i64> = HashMap::with_capacity(n as usize);
        for i in 0..n {
            m.insert(i * 7, i);
        }
        let sum_insert = m.len() as i64;
        for k in 0..n {
            if m.contains_key(&(k * 7)) {
                acc += 1;
            }
        }
        acc += sum_insert;
    }
    if acc == 0 {
        eprintln!("unexpected zero acc");
        std::process::exit(1);
    }
}
