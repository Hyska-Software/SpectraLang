// 08-hashmap-bench (Go): same workload as the Spectra twin.
// 500 inserts + 500 contains per round, 200 rounds, checksum = 100000.
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
		m := make(map[int]int, n)
		for i := 0; i < n; i++ {
			m[i*7] = i
		}
		if len(m) != n {
			fmt.Fprintln(os.Stderr, "map len mismatch")
			os.Exit(1)
		}
		found := 0
		for k := 0; k < n; k++ {
			if _, ok := m[k*7]; ok {
				found++
			}
		}
		total += found
	}
	if total != 100000 {
		fmt.Fprintln(os.Stderr, "checksum mismatch")
		os.Exit(1)
	}
	fmt.Println("HASHMAP ok total=100000")
}
