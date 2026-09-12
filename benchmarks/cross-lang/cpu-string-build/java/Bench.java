// Phase 31: cpu-string-build (Java)

public class Bench {
    public static void main(String[] args) {
        final int iters = 500;
        long total = 0L;
        for (int k = 0; k < iters; k++) {
            StringBuilder b = new StringBuilder(200);
            for (int i = 0; i < 100; i++) {
                b.append("x|");
            }
            total += b.length();
        }
        if (total != 100000L) {
            System.err.println("unexpected: " + total);
            System.exit(1);
        }
    }
}
