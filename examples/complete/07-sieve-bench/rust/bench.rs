// 07-sieve-bench (Rust): same workload as the Spectra twin.
// N=500 (95 primes), iters=200, checksum total = 19000.

fn main() {
    const N: usize = 500;
    const ITERS: i64 = 200;
    let mut total: i64 = 0;
    for _ in 0..ITERS {
        let mut sieve = vec![0i64; N + 1];
        let mut p: usize = 2;
        while p * p <= N {
            if sieve[p] == 0 {
                let mut m = p * p;
                while m <= N {
                    sieve[m] = 1;
                    m += p;
                }
            }
            p += 1;
        }
        let mut count: i64 = 0;
        for k in 2..=N {
            if sieve[k] == 0 {
                count += 1;
            }
        }
        if count != 95 {
            eprintln!("prime count mismatch");
            std::process::exit(1);
        }
        total += count;
    }
    if total != 19000 {
        eprintln!("checksum mismatch");
        std::process::exit(1);
    }
    println!("SIEVE ok primes=95 total=19000");
}
