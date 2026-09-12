// Phase 31: cpu-string-build (Go)

package main

import (
	"fmt"
	"os"
	"strings"
)

func main() {
	const iters = 500
	total := 0
	for k := 0; k < iters; k++ {
		var b strings.Builder
		b.Grow(200)
		for i := 0; i < 100; i++ {
			b.WriteString("x|")
		}
		total += b.Len()
	}
	if total != 100000 {
		fmt.Fprintf(os.Stderr, "unexpected: %d\n", total)
		os.Exit(1)
	}
}
