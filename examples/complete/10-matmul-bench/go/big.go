// 10-matmul-bench (Go): same workload as the Spectra twin.
// Naive 32x32 matmul, 20 rounds, loop order i,j,k.
// Checksum total = 55869440.
package main

import (
	"fmt"
	"os"
)

func main() {
	const n = 32
	const iters = 400
	total := 0
	for it := 0; it < iters; it++ {
		round := 0
		c00 := 0
		clast := 0
		for i := 0; i < n; i++ {
			for j := 0; j < n; j++ {
				cell := 0
				for k := 0; k < n; k++ {
					cell += (i + k) * (k - j)
				}
				if i == 0 && j == 0 {
					c00 = cell
				}
				if i == 31 && j == 31 {
					clast = cell
				}
				round += cell
			}
		}
		if c00 != 10416 || clast != -20336 || round != 2793472 {
			fmt.Fprintln(os.Stderr, "checksum mismatch in round", it)
			os.Exit(1)
		}
		total += round
	}
	if total != 1117388800 {
		fmt.Fprintln(os.Stderr, "checksum mismatch")
		os.Exit(1)
	}
	fmt.Println("MATMUL ok n=32 sum=1117388800")
}
