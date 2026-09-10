// 10-matmul-bench (Rust): same workload as the Spectra twin.
// Naive 32x32 matmul, 20 rounds, loop order i,j,k.
// Checksum total = 55869440.

fn main() {
    const N: i64 = 32;
    const ITERS: i64 = 400;
    let mut total: i64 = 0;
    for _ in 0..ITERS {
        let mut round: i64 = 0;
        let mut c00: i64 = 0;
        let mut clast: i64 = 0;
        for i in 0..N {
            for j in 0..N {
                let mut cell: i64 = 0;
                for k in 0..N {
                    cell += (i + k) * (k - j);
                }
                if i == 0 && j == 0 {
                    c00 = cell;
                }
                if i == 31 && j == 31 {
                    clast = cell;
                }
                round += cell;
            }
        }
        if c00 != 10416 || clast != -20336 || round != 2793472 {
            eprintln!("checksum mismatch in round");
            std::process::exit(1);
        }
        total += round;
    }
    if total != 1117388800 {
        eprintln!("checksum mismatch");
        std::process::exit(1);
    }
    println!("MATMUL ok n=32 sum=1117388800");
}
