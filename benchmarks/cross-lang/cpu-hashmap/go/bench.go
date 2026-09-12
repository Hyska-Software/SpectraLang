// Phase 31: cpu-hashmap (Go)

package main

import (
	"fmt"
	"os"
)

func main() {
	const n = 200
	const iters = 300
	acc := 0
	for it := 0; it < iters; it++ {
		m := make(map[int]int, n)
		for i := 0; i < n; i++ {
			m[i*7] = i
		}
		sumInsert := len(m)
		for k := 0; k < n; k++ {
			if _, ok := m[k*7]; ok {
				acc++
			}
		}
		acc += sumInsert
	}
	if acc == 0 {
		fmt.Fprintln(os.Stderr, "unexpected zero acc")
		os.Exit(1)
	}
}
