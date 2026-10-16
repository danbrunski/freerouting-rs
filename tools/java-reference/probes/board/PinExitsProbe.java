import app.freerouting.Freerouting;
import app.freerouting.board.facade.RoutingBoard;
import app.freerouting.board.model.items.Item;
import app.freerouting.board.model.items.Pin;
import app.freerouting.board.model.structure.Component;
import app.freerouting.core.RoutingJob;
import app.freerouting.geometry.planar.Direction;
import app.freerouting.geometry.planar.FloatPoint;
import app.freerouting.geometry.planar.IntDirection;
import app.freerouting.geometry.planar.IntPoint;
import app.freerouting.geometry.planar.Point;
import app.freerouting.geometry.planar.Polyline;
import app.freerouting.io.specctra.SesReader;
import app.freerouting.management.BoardLoader;
import app.freerouting.settings.GlobalSettings;
import java.io.FileInputStream;
import java.io.InputStream;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.Collection;
import java.util.Random;
import java.util.Set;
import java.util.UUID;

/**
 * Board probe {@code pin-exits} (B05.3, B05.4): per pin of the loaded board (no session), in
 * item-list order, a line with {@code isPlacedOnFront} and {@code hasTraceExitRestrictions}, then
 * per layer from the pin's first to its last a line with {@code getTraceExitRestrictions} (each
 * direction's vector and {@code minLength}), {@code getMinWidth}, {@code getMaxWidth}, {@code
 * getTraceNeckdownHalfwidth}, and for 5 points per pin ({@code Random(3)}, the pin centre plus
 * up to 5000 on each axis) {@code nearestTraceExitCorner(point, 125, layer)} and {@code
 * calcNearestExitRestrictionDirection} of the 2-point polyline from the centre to the point.
 * Runs on {@link #BOARDS} and on {@link #BACK}, the first {@code fixtures.tsv} row with a
 * component on the back side, which {@code --find-back fixtures.tsv} finds; exits 3 on the
 * others. The first line after the header names the board's role.
 *
 * <pre>
 * tools/java-reference/probe.sh &lt;pin&gt; tools/java-reference/probes/board/PinExitsProbe.java \
 *     board.dsn board.ses|- out.jsonl
 * </pre>
 */
public class PinExitsProbe {
  static final Set<String> BOARDS =
      Set.of(
          "Issue955-rotated-pad-contacts", "Issue145-smoothieboard", "Issue026-J2_reference");
  static final String BACK = "Issue022-AutoRouter_interrupted";
  static final int HALF_WIDTH = 125;

  public static void main(String[] args) throws Exception {
    if (args[0].equals("--find-back")) {
      findBack(args[1]);
      return;
    }
    String name = Boards.board(args[0]);
    if (!BOARDS.contains(name) && !name.equals(BACK)) {
      System.exit(3);
    }
    Boards.Out out = new Boards.Out("pin-exits", args[0]);
    try {
      RoutingBoard board = Boards.load(args[0]);
      out.line(
          new Boards.Line()
              .str("stage", "pins")
              .str("role", BOARDS.contains(name) ? "listed" : "first-back-side")
              .num("pinCount", board.getPins().size())
              .end());
      pins(out, board);
    } catch (Throwable t) {
      out.exception(t);
    }
    out.write(args[2]);
  }

  static boolean hasBackSide(RoutingBoard board) {
    for (int i = 1; i <= board.components.count(); i++) {
      Component c = board.components.get(i);
      if (c.isPlaced() && !c.placedOnFront() && !board.getComponentPins(c.id).isEmpty()) {
        return true;
      }
    }
    return false;
  }

  /** Prints the first fixtures.tsv board with a placed back-side component that has pins. */
  static void findBack(String tsv) throws Exception {
    for (String line : Files.readAllLines(Path.of(tsv))) {
      if (line.isEmpty() || line.startsWith("#")) {
        continue;
      }
      String dsn = line.split("\t")[0];
      if (hasBackSide(Boards.load(dsn))) {
        System.out.println(Boards.board(dsn));
        return;
      }
    }
  }

  static String dir(Direction d) {
    if (d == null) {
      return "null";
    }
    if (d instanceof IntDirection i) {
      return i.x + "," + i.y;
    }
    return "?" + d;
  }

  static String fp(FloatPoint p) {
    return p == null ? "null" : Boards.bits(p.x) + "," + Boards.bits(p.y);
  }

  static void pins(Boards.Out out, RoutingBoard board) {
    Random random = new Random(3);
    for (Item item : board.getItems()) {
      if (!(item instanceof Pin pin)) {
        continue;
      }
      IntPoint center = (IntPoint) pin.getCenter();
      IntPoint[] points = new IntPoint[5];
      for (int i = 0; i < points.length; i++) {
        points[i] =
            new IntPoint(center.x + random.nextInt(10001) - 5000, center.y + random.nextInt(10001) - 5000);
      }
      out.line(
          new Boards.Line()
              .str("pin", pin.getId() + "")
              .bool("front", pin.isPlacedOnFront())
              .bool("restricted", pin.hasTraceExitRestrictions())
              .num("first", pin.firstLayer())
              .num("last", pin.lastLayer())
              .end());
      for (int layer = pin.firstLayer(); layer <= pin.lastLayer(); layer++) {
        Collection<Pin.TraceExitRestriction> exits = pin.getTraceExitRestrictions(layer);
        StringBuilder exitText = new StringBuilder();
        for (Pin.TraceExitRestriction exit : exits) {
          exitText
              .append(exitText.length() > 0 ? ";" : "")
              .append(dir(exit.direction))
              .append(',')
              .append(Boards.bits(exit.minLength));
        }
        StringBuilder corners = new StringBuilder();
        StringBuilder directions = new StringBuilder();
        for (IntPoint p : points) {
          corners
              .append(corners.length() > 0 ? ";" : "")
              .append(fp(pin.nearestTraceExitCorner(p.toFloat(), HALF_WIDTH, layer)));
          String d = "same";
          if (!p.equals(center)) {
            d = dir(
                pin.calcNearestExitRestrictionDirection(
                    new Polyline(new Point[] {center, p}), HALF_WIDTH, layer));
          }
          directions.append(directions.length() > 0 ? ";" : "").append(d);
        }
        out.line(
            new Boards.Line()
                .str("pin", pin.getId() + "")
                .num("layer", layer)
                .str("exits", exitText.toString())
                .dbl("minWidth", pin.getMinWidth(layer))
                .dbl("maxWidth", pin.getMaxWidth(layer))
                .num("neckdown", pin.getTraceNeckdownHalfwidth(layer))
                .str("corners", corners.toString())
                .str("directions", directions.toString())
                .end());
      }
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
