import app.freerouting.Freerouting;
import app.freerouting.board.facade.RoutingBoard;
import app.freerouting.board.model.items.Item;
import app.freerouting.board.model.items.Trace;
import app.freerouting.board.model.items.Via;
import app.freerouting.board.searchtree.ShapeTraceEntries;
import app.freerouting.board.trace.PolylineTrace;
import app.freerouting.core.RoutingJob;
import app.freerouting.drc.DesignRulesChecker;
import app.freerouting.geometry.planar.IntBox;
import app.freerouting.geometry.planar.IntPoint;
import app.freerouting.geometry.planar.Point;
import app.freerouting.io.specctra.SesReader;
import app.freerouting.management.BoardLoader;
import app.freerouting.settings.GlobalSettings;
import java.io.FileInputStream;
import java.io.InputStream;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.List;
import java.util.Random;
import java.util.Set;
import java.util.UUID;

/**
 * Board probe {@code ledger} (B01.6, Java only): where Java's {@code NetRoutingLedger} goes
 * stale. After each step it logs {@code routingLedger().incompleteCount()} and a fresh full count
 * ({@code DesignRulesChecker.getIncompleteCount()}). Steps: load (the first read builds the
 * ledger); session import; {@code generateSnapshot}, unfix and remove the 10 highest-id traces, undo; {@code assignNetNo}
 * on 5 vias; {@code ShapeTraceEntries.cutoutTrace} on 20 (trace, octagon) pairs drawn with {@code
 * new Random(1)}. Runs on the boards in {@link #BOARDS} only and exits 3 on the others.
 *
 * <pre>
 * tools/java-reference/probe.sh &lt;pin&gt; tools/java-reference/probes/board/LedgerProbe.java \
 *     board.dsn board.ses out.jsonl
 * </pre>
 */
public class LedgerProbe {
  static final Set<String> BOARDS =
      Set.of("Issue026-J2_reference", "Issue508-DAC2020_bm08", "Issue413-test");

  public static void main(String[] args) throws Exception {
    if (!BOARDS.contains(Boards.board(args[0])) || args[1].equals("-")) {
      System.exit(3);
    }
    Boards.Out out = new Boards.Out("ledger", args[0]);
    try {
      RoutingBoard board = Boards.load(args[0]);
      step(out, "load", board);
      Boards.importSession(board, args[1]);
      step(out, "ses", board);
      board.generateSnapshot();
      List<Trace> traces = new ArrayList<>(board.getTraces());
      for (Trace trace : traces.subList(0, Math.min(10, traces.size()))) {
        // The session's wires are user fixed, which removeItem refuses.
        trace.unfix();
        board.removeItem(trace);
      }
      step(out, "remove-10-traces", board);
      board.undo(null);
      step(out, "undo", board);
      int maxNet = board.rules.nets.maxNetNumber();
      List<Via> vias = new ArrayList<>(board.getVias());
      for (int i = 0; i < Math.min(5, vias.size()); i++) {
        vias.get(i).assignNetNo(i % Math.max(1, maxNet) + 1);
      }
      step(out, "assign-net-5-vias", board);
      Random random = new Random(1);
      for (int i = 0; i < 20; i++) {
        List<Trace> current = new ArrayList<>(board.getTraces());
        if (current.isEmpty()) {
          break;
        }
        Trace picked = current.get(random.nextInt(current.size()));
        if (!(picked instanceof PolylineTrace trace)) {
          continue;
        }
        int corner = random.nextInt(trace.cornerCount());
        Point point = trace.polyline().corner(corner);
        IntPoint center = point.toFloat().round();
        int radius = 2 * trace.getHalfWidth() + random.nextInt(4 * trace.getHalfWidth() + 1);
        IntBox box =
            new IntBox(
                center.x - radius, center.y - radius, center.x + radius, center.y + radius);
        ShapeTraceEntries.cutoutTrace(trace, box.boundingOctagon(), 0);
        step(out, "cutout-" + i, board);
      }
    } catch (Throwable t) {
      out.exception(t);
    }
    out.write(args[2]);
  }

  static void step(Boards.Out out, String name, RoutingBoard board) {
    int ledger = board.routingLedger().incompleteCount();
    DesignRulesChecker checker = new DesignRulesChecker(board, null);
    int fresh = checker.getIncompleteCount();
    out.line(
        new Boards.Line()
            .str("stage", name)
            .num("maxGeneratedId", board.communication.idGenerator.maxGeneratedId())
            .num("items", board.getItems().size())
            .num("ledger", ledger)
            .num("fresh", fresh)
            .bool("stale", ledger != fresh)
            .end());
  }

  // BEGIN Boards
  /**
   * The board-probe support every B probe copies verbatim (B00 "Board probes"). Loads a board as
   * {@code cli/DrcRunner} does, waits for the post-load processing, imports a session with {@code
   * SesReader}, and writes the probe's JSONL (fixed key order, decimal integers, doubles as raw
   * bits in hex).
   */
  static final class Boards {
    static final long FNV_OFFSET = 0xcbf29ce484222325L;
    static final long FNV_PRIME = 0x100000001b3L;

    /** Loads {@code dsn} as {@code DrcRunner.initializeDrc} does and waits for the post-load. */
    static RoutingBoard load(String dsn) throws Exception {
      Freerouting.globalSettings = new GlobalSettings();
      RoutingJob job = new RoutingJob(new UUID(0L, 1L));
      job.setInput(dsn);
      if (!BoardLoader.loadBoardIfNeeded(job)) {
        throw new IllegalStateException("BoardLoader.loadBoardIfNeeded failed");
      }
      job.board.awaitPostLoad();
      return job.board;
    }

    /** Imports the session {@code ses} into {@code board} with Java's {@code SesReader}. */
    static void importSession(RoutingBoard board, String ses) throws Exception {
      try (InputStream in = new FileInputStream(ses)) {
        SesReader.read(in, board);
      }
    }

    static long fnv(long hash, String text) {
      for (byte b : text.getBytes(StandardCharsets.UTF_8)) {
        hash ^= (b & 0xff);
        hash *= FNV_PRIME;
      }
      return hash;
    }

    static String hex(long value) {
      return String.format("%016x", value);
    }

    static String bits(double value) {
      return "0x" + String.format("%016x", Double.doubleToRawLongBits(value));
    }

    /** The DSN path below {@code fixtures/} without {@code .dsn}, as fixtures.tsv names it. */
    static String board(String dsn) {
      String name = dsn.replace('\\', '/');
      int fixtures = name.lastIndexOf("fixtures/");
      if (fixtures >= 0) {
        name = name.substring(fixtures + "fixtures/".length());
      }
      return name.endsWith(".dsn") ? name.substring(0, name.length() - 4) : name;
    }

    /** One JSON object; keys are written in call order. */
    static final class Line {
      private final StringBuilder text = new StringBuilder("{");

      private Line key(String key) {
        if (text.length() > 1) {
          text.append(',');
        }
        text.append('"').append(key).append("\":");
        return this;
      }

      Line num(String key, long value) {
        key(key).text.append(value);
        return this;
      }

      Line dbl(String key, double value) {
        return str(key, bits(value));
      }

      Line bool(String key, boolean value) {
        key(key).text.append(value);
        return this;
      }

      Line raw(String key, String json) {
        key(key).text.append(json);
        return this;
      }

      Line str(String key, String value) {
        key(key).text.append('"');
        for (char c : value.toCharArray()) {
          if (c == '"' || c == '\\') {
            text.append('\\').append(c);
          } else if (c < 0x20) {
            text.append(String.format("\\u%04x", (int) c));
          } else {
            text.append(c);
          }
        }
        text.append('"');
        return this;
      }

      String end() {
        return text.append('}').toString();
      }
    }

    /** The probe output: the header line, then the probe's lines. */
    static final class Out {
      private final StringBuilder text = new StringBuilder();

      Out(String kind, String dsn) {
        line(
            new Line()
                .str("kind", kind)
                .str("board", board(dsn))
                .str("javaCommit", app.freerouting.constants.Constants.FREEROUTING_VERSION)
                .str("jdk", System.getProperty("java.version"))
                .num("format", 1)
                .end());
      }

      void line(String line) {
        text.append(line).append('\n');
      }

      /** A Java exception as a record; the probe stops after it. */
      void exception(Throwable t) {
        StackTraceElement[] trace = t.getStackTrace();
        String at = trace.length > 0 ? trace[0].toString() : "";
        line(new Line().str("exception", t.toString()).str("at", at).end());
      }

      void write(String path) throws Exception {
        Path target = Path.of(path);
        if (target.getParent() != null) {
          Files.createDirectories(target.getParent());
        }
        Files.writeString(target, text.toString(), StandardCharsets.UTF_8);
      }
    }
  }
  // END Boards
}
