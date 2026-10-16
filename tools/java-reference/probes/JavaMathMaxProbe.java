/**
 * Java `Math.max(float, float)` and `Math.max(double, double)` on signed zeros, NaN and
 * infinities: one line per pair, `f|d <a bits> <b bits> <max bits>` in hex. The table in
 * `freerouting-geometry/src/jmath.rs` (`max_matches_java_math_max`) is its output at a773a70ec.
 */
public class JavaMathMaxProbe {
  public static void main(String[] args) {
    float[] f = {0.0f, -0.0f, 1.5f, -1.5f, Float.NaN, Float.POSITIVE_INFINITY, Float.NEGATIVE_INFINITY};
    for (float a : f) for (float b : f) {
      System.out.println("f " + Integer.toHexString(Float.floatToRawIntBits(a)) + " " + Integer.toHexString(Float.floatToRawIntBits(b)) + " " + Integer.toHexString(Float.floatToRawIntBits(Math.max(a, b))));
    }
    double[] d = {0.0, -0.0, 1.5, -1.5, Double.NaN, Double.POSITIVE_INFINITY, Double.NEGATIVE_INFINITY};
    for (double a : d) for (double b : d) {
      System.out.println("d " + Long.toHexString(Double.doubleToRawLongBits(a)) + " " + Long.toHexString(Double.doubleToRawLongBits(b)) + " " + Long.toHexString(Double.doubleToRawLongBits(Math.max(a, b))));
    }
  }
}
