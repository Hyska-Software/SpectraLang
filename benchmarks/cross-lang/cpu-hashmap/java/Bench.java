// Phase 31: cpu-hashmap (Java)

import java.util.HashMap;
import java.util.Map;

public class Bench {
    public static void main(String[] args) {
        final int n = 200;
        final int iters = 300;
        long acc = 0L;
        for (int it = 0; it < iters; it++) {
            Map<Integer, Integer> m = new HashMap<>(n);
            for (int i = 0; i < n; i++) {
                m.put(i * 7, i);
            }
            int sumInsert = m.size();
            for (int k = 0; k < n; k++) {
                if (m.containsKey(k * 7)) {
                    acc++;
                }
            }
            acc += sumInsert;
        }
        if (acc == 0L) {
            System.err.println("unexpected zero acc");
            System.exit(1);
        }
    }
}
