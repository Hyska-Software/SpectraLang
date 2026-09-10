// Phase 31: tensor-create (Go)

package main

import (
	"fmt"
	"os"
)

func main() {
	const iters = 60
	total := 0
	for i := 0; i < iters; i++ {
		t := make([]float64, 1_048_576)
		for j := range t {
			t[j] = 1.0
		}
		total += len(t)
	}
	if total != 62_914_560 {
		fmt.Fprintf(os.Stderr, "unexpected: %d\n", total)
		os.Exit(1)
	}
}
