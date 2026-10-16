import app.freerouting.Freerouting;
import app.freerouting.board.facade.RoutingBoard;
import app.freerouting.board.searchtree.ShapeSearchTree;
import app.freerouting.core.RoutingJob;
import app.freerouting.datastructures.ShapeTree;
import app.freerouting.datastructures.WorkMeter;
import app.freerouting.geometry.planar.IntBox;
import app.freerouting.geometry.planar.IntOctagon;
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
 * Board probe {@code tree-visits} (B02.7): the work units ({@code WorkMeter}, search-tree nodes
 * visited) of overlap queries on the default tree, which Java's work limits count since
 * {@code 30baad7e2}. Needs a pin with {@code WorkMeter} (J01's re-pin, {@code a773a70ec}). Per
 * stage ({@code load}, then {@code ses}) 1 000 queries seeded with {@code Random(7)}: an
 * {@code IntOctagon} (a box) around a seeded point of the board's bounding box, with a seeded
 * half size of 1 up to 1/20 of the box's smaller side, through {@code
 * ShapeSearchTree.overlappingTreeEntries(shape, -1, list)}. One line per query with the units
 * before and after it (as a delta) and the entries found, then the stage's sum. Runs on the boards
 * in {@link #BOARDS} only and exits 3 on the others.
 *
 * <pre>
 * tools/java-reference/probe.sh &lt;pin&gt; tools/java-reference/probes/board/TreeVisitsProbe.java \
 *     board.dsn board.ses out.jsonl
 * </pre>
 */
public class TreeVisitsProbe {
  static final Set<String> BOARDS =
      Set.of(
          "Issue026-J2_reference",
          "Issue508-DAC2020_bm08",
          "Issue145-smoothieboard",
          "Issue190-processor.Z80");

  public static void main(String[] args) throws Exception {
    if (!BOARDS.contains(Boards.board(args[0])) || args[1].equals("-")) {
      System.exit(3);
    }
    Boards.Out out = new Boards.Out("tree-visits", args[0]);
    try {
      RoutingBoard board = Boards.load(args[0]);
      queries(out, "load", board);
      Boards.importSession(board, args[1]);
      queries(out, "ses", board);
    } catch (Throwable t) {
      out.exception(t);
    }
    out.write(args[2]);
  }

  static void queries(Boards.Out out, String stage, RoutingBoard board) {
    ShapeSearchTree tree = board.searchTreeManager.getDefaultTree();
    IntBox box = board.getBoundingBox();
    int width = Math.max(1, box.ur.x - box.ll.x);
    int height = Math.max(1, box.ur.y - box.ll.y);
    int maxHalf = Math.max(1, Math.min(width, height) / 20);
    Random random = new Random(7);
    WorkMeter meter = WorkMeter.current();
    long sum = 0;
    List<String> lines = new ArrayList<>();
    for (int q = 0; q < 1000; q++) {
      int x = box.ll.x + random.nextInt(width);
      int y = box.ll.y + random.nextInt(height);
      int half = 1 + random.nextInt(maxHalf);
      IntOctagon shape = new IntBox(x - half, y - half, x + half, y + half).toIntOctagon();
      List<ShapeTree.TreeEntry> found = new ArrayList<>();
      long before = meter.units();
      tree.overlappingTreeEntries(shape, -1, found);
      long units = meter.units() - before;
      sum += units;
      lines.add(
          new Boards.Line()
              .str("stage", stage)
              .num("q", q)
              .num("units", units)
              .num("found", found.size())
              .end());
    }
    out.line(new Boards.Line().str("stage", stage).num("queries", 1000).num("sum", sum).end());
    for (String line : lines) {
      out.line(line);
    }
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
