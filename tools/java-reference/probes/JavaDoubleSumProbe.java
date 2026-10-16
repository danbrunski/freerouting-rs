import java.util.Random;
import java.util.stream.DoubleStream;

/**
 * {@code DoubleStream.sum()} (compensated summation) against a plain left-to-right sum, on
 * values from {@code new Random(seed)}: {@code nextDouble() * 10^nextInt(12)}, negated when
 * {@code nextBoolean()}. One line per (seed, count): {@code <seed> <count> <stream sum bits>
 * <plain sum bits>} in hex. The table in {@code freerouting-geometry/src/jmath.rs}
 * ({@code double_stream_sum_matches_java}) is its output at a773a70ec on JDK 25.
 */
public class JavaDoubleSumProbe {
  public static void main(String[] args) {
    int[] counts = {0, 1, 2, 3, 10, 100, 1000};
    for (long seed = 1; seed <= 4; seed++) {
      for (int count : counts) {
        Random random = new Random(seed * 1000 + count);
        double[] values = new double[count];
        double plain = 0.0;
        for (int i = 0; i < count; i++) {
          double v = random.nextDouble() * Math.pow(10, random.nextInt(12));
          values[i] = random.nextBoolean() ? -v : v;
          plain += values[i];
        }
        double sum = DoubleStream.of(values).sum();
        System.out.println(
            seed + " " + count + " " + Long.toHexString(Double.doubleToRawLongBits(sum)) + " "
                + Long.toHexString(Double.doubleToRawLongBits(plain)));
      }
    }
  }
}
