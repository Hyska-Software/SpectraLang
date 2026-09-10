// Phase 31: async-echo (Java)

import java.util.concurrent.atomic.AtomicLong;

public class Bench {
    public static void main(String[] args) throws Exception {
        final int iters = 10000;
        long total = 0L;
        for (int i = 0; i < iters; i++) {
            AtomicLong local = new AtomicLong(0L);
            Thread[] threads = new Thread[10];
            for (int k = 0; k < 10; k++) {
                final int v = k + 1;
                threads[k] = new Thread(() -> local.addAndGet(v));
                threads[k].start();
            }
            for (int k = 0; k < 10; k++) {
                threads[k].join();
            }
            total += local.get();
        }
        if (total != 550_000L) {
            System.err.println("unexpected: " + total);
            System.exit(1);
        }
    }
}
