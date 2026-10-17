import app.freerouting.board.actions.ItemIdGenerator;
import app.freerouting.board.facade.BasicBoard;
import app.freerouting.board.model.items.Item;
import app.freerouting.board.model.structure.FixedState;
import app.freerouting.board.trace.PolylineTrace;
import app.freerouting.geometry.planar.IntPoint;
import app.freerouting.geometry.planar.Point;
import app.freerouting.geometry.planar.Polyline;
import app.freerouting.io.BoardReadResult;
import app.freerouting.io.specctra.DsnReader;
import java.io.ByteArrayInputStream;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.Arrays;

/**
 * The board after a normalization that removes a trace and reports no change
 * ({@code crates/freerouting/tests/normalize_stale_trace.rs}). Prints {@code maxGeneratedId}, then
 * one line per item in item-list order: id, class, nets, and for a trace its layer, half width and
 * corners.
 *
 * <pre>
 * tools/java-reference/probe.sh &lt;pin-checkout&gt; tools/java-reference/probes/NormalizeStaleTraceProbe.java \
 *     crates/freerouting/tests/data/normalize-stale-trace.dsn load|insert
 * </pre>
 *
 * <p>{@code load} reads the DSN ({@code DsnReader.readBoard}, which runs {@code
 * normalizeAllTraces}). {@code insert} reads it without its {@code (wiring ...)} section, inserts
 * the wire's polyline with {@code insertTraceWithoutCleaning} (layer 0, half width 2500, net 1,
 * clearance class 1, unfixed) and calls {@code normalizeTraces(1)}, whose result it prints first.
 */
public class NormalizeStaleTraceProbe {
  static final int[][] WIRE = {
    {1246000, -650750}, {1246000, -650749}, {1230000, -650750},
    {1293500, -650750}, {1278000, -650750}, {1278000, -650749}
  };

  public static void main(String[] args) throws Exception {
    String text = Files.readString(Path.of(args[0]), StandardCharsets.UTF_8);
    boolean insert = args[1].equals("insert");
    if (insert) {
      int start = text.indexOf("  (wiring");
      text = text.substring(0, start) + ")\n";
    }
    BoardReadResult read =
        DsnReader.readBoard(
            new ByteArrayInputStream(text.getBytes(StandardCharsets.UTF_8)),
            null,
            new ItemIdGenerator(),
            args[0]);
    BasicBoard board =
        switch (read) {
          case BoardReadResult.Success s -> s.board();
          case BoardReadResult.OutlineMissing s -> s.board();
          default -> throw new IllegalStateException(read.toString());
        };
    if (insert) {
      Point[] corners = new Point[WIRE.length];
      for (int i = 0; i < WIRE.length; i++) {
        corners[i] = new IntPoint(WIRE[i][0], WIRE[i][1]);
      }
      board.insertTraceWithoutCleaning(
          new Polyline(corners), 0, 2500, new int[] {1}, 1, FixedState.UNFIXED);
      System.out.println("normalizeTraces " + board.normalizeTraces(1));
    }
    System.out.println("maxGeneratedId " + board.communication.idGenerator.maxGeneratedId());
    for (Item item : board.getItems()) {
      StringBuilder line = new StringBuilder();
      line.append(item.getId())
          .append(' ')
          .append(item.getClass().getSimpleName())
          .append(" nets ")
          .append(Arrays.toString(item.netNumbers));
      if (item instanceof PolylineTrace trace) {
        line.append(" layer ")
            .append(trace.getLayer())
            .append(" half_width ")
            .append(trace.getHalfWidth())
            .append(" corners");
        Polyline polyline = trace.polyline();
        for (int i = 0; i < polyline.cornerCount(); i++) {
          Point corner = polyline.corner(i);
          if (corner instanceof IntPoint p) {
            line.append(' ').append(p.x).append(',').append(p.y);
          } else {
            line.append(' ').append(corner);
          }
        }
      }
      System.out.println(line);
    }
  }
}
