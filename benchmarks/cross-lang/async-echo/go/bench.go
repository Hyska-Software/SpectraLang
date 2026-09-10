// Phase 31: async-echo (Go)
// Contract: fanout_fanin_real_concurrency.v2.

package main

import (
	"fmt"
	"os"
	"sync"
)

func main() {
	const iters = 10000
	total := 0
	for i := 0; i < iters; i++ {
		var wg sync.WaitGroup
		var local int64
		var mu sync.Mutex
		for k := 1; k <= 10; k++ {
			wg.Add(1)
			go func(v int) {
				defer wg.Done()
				mu.Lock()
				local += int64(v)
				mu.Unlock()
			}(k)
		}
		wg.Wait()
		total += int(local)
	}
	if total != 550000 {
		fmt.Fprintf(os.Stderr, "unexpected: %d\n", total)
		os.Exit(1)
	}
}
