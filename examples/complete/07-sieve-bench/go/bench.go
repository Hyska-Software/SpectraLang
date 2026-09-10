// 07-sieve-bench (Go): same workload as the Spectra twin.
// N=500 (95 primes), iters=200, checksum total = 19000.
package main

import (
	"fmt"
	"os"
)

func main() {
	const n = 500
	const iters = 200
	total := 0
	for it := 0; it < iters; it++ {
		sieve := make([]int, n+1)
		for p := 2; p*p <= n; p++ {
			if sieve[p] == 0 {
				for m := p * p; m <= n; m += p {
					sieve[m] = 1
				}
			}
		}
		count := 0
		for k := 2; k <= n; k++ {
			if sieve[k] == 0 {
				count++
			}
		}
		if count != 95 {
			fmt.Fprintln(os.Stderr, "prime count mismatch")
			os.Exit(1)
		}
		total += count
	}
	if total != 19000 {
		fmt.Fprintln(os.Stderr, "checksum mismatch")
		os.Exit(1)
	}
	fmt.Println("SIEVE ok primes=95 total=19000")
}
