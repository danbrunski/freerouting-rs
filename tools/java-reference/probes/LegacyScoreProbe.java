import app.freerouting.core.scoring.BoardStatistics;
import app.freerouting.settings.RoutingCostSettings;

/**
 * The legacy board score (B06.1) on hand-made statistics: per case one line
 * {@code <case> <getMaximumScore> <calculateScore> <getRouterScore(RoutingCostSettings)>} with the
 * raw float bits in hex. {@code crates/freerouting/tests/scores.rs} holds the output at
 * a773a70ec.
 *
 * <pre>
 * tools/java-reference/probe.sh &lt;pin&gt; tools/java-reference/probes/LegacyScoreProbe.java
 * </pre>
 */
public class LegacyScoreProbe {
  // maximum, incomplete, violations, bends, vias, lengthMm, unrouted, clearance, bend, viaCosts,
  // traceCost
  static final Object[][] CASES = {
    {"defaults", 10, 2, 1, 7, 3, 123.5f, 5_000_000f, 1_000_000f, 10f, 50, 1.0},
    {"split", 1, 0, 0, 0, 1, 0.1f, 5_000_000f, 1_000_000f, 10f, 16_777_217, 0.3},
    {"fraction", 37, 5, 0, 113, 21, 987.654f, 5_000_000f, 1_000_000f, 10f, 50, 1.0},
    {"doubled", 37, 5, 0, 113, 21, 987.654f, 5_000_000f, 1_000_000f, 20f, 100, 2.0},
    {"negative", 1, 1, 2, 3, 4, 10.25f, 5_000_000f, 1_000_000f, 10f, 50, 1.0},
    {"via-wrap", 3, 0, 0, 0, 50_000_000, 0f, 5_000_000f, 1_000_000f, 10f, 50, 1.0},
    {"no-connections", 0, 0, 0, 2, 1, 5f, 5_000_000f, 1_000_000f, 10f, 50, 1.0},
    {"zero-penalty", 4, 1, 0, 2, 1, 5f, 0f, 1_000_000f, 10f, 50, 1.0},
    {"negative-penalty", 3, 1, 0, 2, 1, 5f, -1f, 1_000_000f, 10f, 50, 1.0},
  };

  public static void main(String[] args) {
    for (Object[] c : CASES) {
      BoardStatistics s = new BoardStatistics();
      s.connections.maximumCount = (Integer) c[1];
      s.connections.incompleteCount = (Integer) c[2];
      s.clearanceViolations.totalCount = (Integer) c[3];
      s.bends.totalCount = (Integer) c[4];
      s.vias.totalCount = (Integer) c[5];
      s.traces.totalLengthMm = (Float) c[6];
      s.traces.totalLength = 0f;
      RoutingCostSettings r = new RoutingCostSettings();
      r.unroutedNetPenalty = (Float) c[7];
      r.clearanceViolationPenalty = (Float) c[8];
      r.bendPenalty = (Float) c[9];
      r.viaCosts = (Integer) c[10];
      r.defaultPreferredDirectionTraceCost = (Double) c[11];
      System.out.println(
          c[0]
              + " "
              + bits(s.getMaximumScore(r))
              + " "
              + bits(s.calculateScore(r))
              + " "
              + bits(s.getRouterScore(r)));
    }
  }

  static String bits(float f) {
    return String.format("%08x", Float.floatToRawIntBits(f));
  }
}
