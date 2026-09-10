// 08-hashmap-bench (Rust): same workload as the Spectra twin.
// 500 inserts + 500 contains per round, 200 rounds, checksum = 100000.

use std::collections::HashMap;

fn main() {
    const N: i64 = 500;
    const ITERS: i64 = 2000;
    let mut total: i64 = 0;
    for _ in 0..ITERS {
        let mut m: HashMap<i64, i64> = HashMap::with_capacity(N as usize);
        for i in 0..N {
            m.insert(i * 7, i);
        }
        if m.len() as i64 != N {
            eprintln!("map len mismatch");
            std::process::exit(1);
        }
        let mut found: i64 = 0;
        for k in 0..N {
            if m.contains_key(&(k * 7)) {
                found += 1;
            }
        }
        total += found;
    }
    if total != 1000000 {
        eprintln!("checksum mismatch");
        std::process::exit(1);
    }
    println!("HASHMAP ok total=1000000");
}
