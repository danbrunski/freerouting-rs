import app.freerouting.Freerouting;
import app.freerouting.board.facade.RoutingBoard;
import app.freerouting.board.model.items.Item;
import app.freerouting.board.model.items.Pin;
import app.freerouting.board.model.items.Via;
import app.freerouting.board.model.structure.AngleRestriction;
import app.freerouting.board.trace.PolylineTrace;
import app.freerouting.core.RoutingJob;
import app.freerouting.geometry.planar.FloatPoint;
import app.freerouting.geometry.planar.IntPoint;
import app.freerouting.geometry.planar.IntVector;
import app.freerouting.geometry.planar.Point;
import app.freerouting.geometry.planar.Polyline;
import app.freerouting.io.specctra.SesReader;
import app.freerouting.management.BoardLoader;
import app.freerouting.rules.NetClass;
import app.freerouting.rules.ViaInfo;
import app.freerouting.rules.ViaRule;
import app.freerouting.settings.GlobalSettings;
import java.io.FileInputStream;
import java.io.InputStream;
import java.lang.reflect.Field;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.List;
import java.util.Map;
import java.util.Random;
import java.util.TreeSet;
import java.util.UUID;

/**
 * Board probe {@code shove-insert} (O04.7): the insert half of push-and-shove, mirrored by
 * {@code crates/freerouting/tests/support/shove_insert_stages.rs}.
 *
 * <p>Boards: Issue413-test, Issue676-ch32v-tx118s and Issue957-smoke-ldo-led as loaded; the
 * others with their fixtures.tsv (routed) session. The probe exits 3 on every other row. One
 * {@code new Random(20261016)} drives all stages, in this order, with the board's own (45
 * degree) restriction:
 *
 * <ul>
 *   <li>{@code insert}: {@value #STEPS} steps (200 on J2) of {@code
 *       board.insertForcedTraceSegment(from, to, halfWidth, layer, {net}, class, 20, 8, 5, tidy,
 *       500, true, null)} with {@code tidy = Integer.MAX_VALUE} for even steps and 0 for odd ones.
 *       {@code from} is a start ({@link #starts}) of a net drawn from the starts' nets
 *       (ascending), {@code to} is {@code from} plus a direction ({@code nextInt(8)} of the 45
 *       degree directions) times {@code (1 + nextInt(60)) * minTraceHalfWidth}; half width and
 *       clearance class are the net class's on the start's layer;
 *   <li>{@code via}: 10 steps of {@code board.forcedVia(firstVia, location, {net}, class, pens, 20,
 *       8, tidy, 500, 0)} with the net class's first via, {@code location} a start plus a direction
 *       times {@code nextInt(21) * minTraceHalfWidth}, {@code pens} the net's half width per
 *       layer, {@code tidy} as above;
 *   <li>{@code move}: 10 steps of {@code board.moveDrillItem(via, vector, 20, 8, tidy, 500, 0)}
 *       on a via drawn from the vias (highest id first), {@code vector} a direction times {@code (1
 *       + nextInt(10)) * minTraceHalfWidth}, {@code tidy = Integer.MAX_VALUE} for even steps and
 *       100 for odd ones;
 *   <li>{@code attach}: at the first SMD pin (one layer, one net, highest id first) whose net
 *       class has a via, {@code forcedVia} with that via copied with attach off, then attach on,
 *       each undone afterwards.
 * </ul>
 *
 * <p>A snapshot is taken before each step and undone after a failed one ({@code null} from the
 * segment insert, {@code false} from the via and move calls), as {@code Route} does. Each step
 * line starts with {@code "stage"} (so a digest has one section per step) and ends with the
 * outcome, the shove-failing obstacle id (-1 for none) and layer and {@code maxGeneratedId}; the
 * canonical item dump follows after every 10th step and at the end of each stage.
 */
public class ShoveInsertProbe {
  static final int STEPS = 30;
  static final int J2_STEPS = 200;
  static final long SEED = 20261016L;
  static final int[][] DIRS45 = {{1, 0}, {1, 1}, {0, 1}, {-1, 1}, {-1, 0}, {-1, -1}, {0, -1}, {1, -1}};
  static final Map<String, Boolean> BOARDS =
      Map.of(
          "Issue413-test", false,
          "Issue676-ch32v-tx118s", false,
          "Issue957-smoke-ldo-led", false,
          "Issue026-J2_reference", true,
          "Issue508-DAC2020_bm05", true,
          "Issue508-DAC2020_bm08", true,
          "Issue143-rpi_splitter", true,
          "Issue508-SMD-routing-issue-demo", true,
          "Issue110-testProjectFromFreeroutingBugTest", true,
          "Issue742-tastexx-pcb", true);

  public static void main(String[] args) throws Exception {
    String board = Boards.board(args[0]);
    Boolean withSession = BOARDS.get(board);
    if (withSession == null) {
      System.exit(3);
    }
    Boards.Out out = new Boards.Out("shove-insert", args[0]);
    try {
      RoutingBoard routingBoard = Boards.load(args[0]);
      if (withSession) {
        Boards.importSession(routingBoard, args[1]);
      }
      Random random = new Random(SEED);
      int steps = board.equals("Issue026-J2_reference") ? J2_STEPS : STEPS;
      long started = System.nanoTime();
      insert(out, routingBoard, random, steps);
      via(out, routingBoard, random);
      move(out, routingBoard, random);
      attach(out, routingBoard);
      // The script's own time (dumps included), on stderr only: the output stays deterministic.
      System.err.printf("shove-insert script: %.1f ms%n", (System.nanoTime() - started) / 1e6);
    } catch (Throwable t) {
      out.exception(t);
    }
    out.write(args[2]);
  }

  /** A start point: a pin centre on the pin's first layer, or a trace end on its layer. */
  static final class Start {
    final IntPoint point;
    final int layer;
    final int net;

    Start(IntPoint point, int layer, int net) {
      this.point = point;
      this.layer = layer;
      this.net = net;
    }
  }

  /**
   * The starts, items highest id first: per pin with exactly one net (positive) and an integer
   * centre, the centre on its first layer; per {@code PolylineTrace} with exactly one net, its
   * first and last corner (if integer) on its layer.
   */
  static List<Start> starts(RoutingBoard board) {
    List<Item> items = new ArrayList<>(board.getItems());
    items.sort((a, b) -> Integer.compare(b.getId(), a.getId()));
    List<Start> result = new ArrayList<>();
    for (Item item : items) {
      if (item.netCount() != 1 || item.getNetNumber(0) <= 0) {
        continue;
      }
      int net = item.getNetNumber(0);
      if (item instanceof Pin pin) {
        if (pin.getCenter() instanceof IntPoint c) {
          result.add(new Start(c, pin.firstLayer(), net));
        }
      } else if (item instanceof PolylineTrace trace) {
        if (trace.firstCorner() instanceof IntPoint c) {
          result.add(new Start(c, trace.getLayer(), net));
        }
        if (trace.lastCorner() instanceof IntPoint c) {
          result.add(new Start(c, trace.getLayer(), net));
        }
      }
    }
    return result;
  }

  /** A net drawn from the starts' nets (ascending), then one of its starts in list order. */
  static Start drawStart(List<Start> starts, Random random) {
    TreeSet<Integer> netSet = new TreeSet<>();
    for (Start s : starts) {
      netSet.add(s.net);
    }
    List<Integer> nets = new ArrayList<>(netSet);
    int net = nets.get(random.nextInt(nets.size()));
    List<Start> ofNet = new ArrayList<>();
    for (Start s : starts) {
      if (s.net == net) {
        ofNet.add(s);
      }
    }
    return ofNet.get(random.nextInt(ofNet.size()));
  }

  static Boards.Line state(Boards.Line line, RoutingBoard board) {
    Item failing = board.getShoveFailingObstacle();
    return line.num("failing", failing == null ? -1 : failing.getId())
        .num("failingLayer", board.getShoveFailingLayer())
        .num("maxGeneratedId", board.communication.idGenerator.maxGeneratedId());
  }

  static void insert(Boards.Out out, RoutingBoard board, Random random, int steps) {
    for (int step = 0; step < steps; step++) {
      List<Start> starts = starts(board);
      if (starts.isEmpty()) {
        out.line(new Boards.Line().str("stage", "insert").num("step", step).str("skip", "no starts").end());
        break;
      }
      Start s = drawStart(starts, random);
      int[] dir = DIRS45[random.nextInt(DIRS45.length)];
      int unit = board.getMinTraceHalfWidth();
      int length = (1 + random.nextInt(60)) * unit;
      IntPoint from = new IntPoint(s.point.x, s.point.y);
      IntPoint to = new IntPoint(s.point.x + dir[0] * length, s.point.y + dir[1] * length);
      int halfWidth = board.rules.getTraceHalfWidth(s.net, s.layer);
      int clearanceClass = board.rules.nets.get(s.net).getNetClass().getTraceClearanceClass();
      int tidy = step % 2 == 0 ? Integer.MAX_VALUE : 0;
      board.generateSnapshot();
      Point result =
          board.insertForcedTraceSegment(
              from, to, halfWidth, s.layer, new int[] {s.net}, clearanceClass, 20, 8, 5, tidy, 500,
              true, null);
      String outcome;
      if (result == null) {
        outcome = "damaged";
        board.undo(null);
      } else if (result == from) {
        outcome = "unchanged";
      } else if (result == to) {
        outcome = "reached";
      } else {
        outcome = "partial " + point(result);
      }
      out.line(
          state(
                  new Boards.Line()
                      .str("stage", "insert")
                      .num("step", step)
                      .num("net", s.net)
                      .num("layer", s.layer)
                      .str("from", point(from))
                      .str("to", point(to))
                      .num("halfWidth", halfWidth)
                      .num("class", clearanceClass)
                      .num("tidy", tidy)
                      .str("outcome", outcome),
                  board)
              .end());
      if (step % 10 == 9) {
        dump(out, board);
      }
    }
    dump(out, board);
  }

  /** The first via of the net class's via rule; null if none. */
  static ViaInfo firstVia(RoutingBoard board, int net) {
    NetClass netClass = board.rules.nets.get(net).getNetClass();
    ViaRule rule = netClass.getViaRule();
    return rule == null || rule.viaCount() == 0 ? null : rule.getVia(0);
  }

  static int[] pens(RoutingBoard board, int net) {
    int[] result = new int[board.getLayerCount()];
    for (int layer = 0; layer < result.length; layer++) {
      result[layer] = board.rules.getTraceHalfWidth(net, layer);
    }
    return result;
  }

  static void via(Boards.Out out, RoutingBoard board, Random random) {
    for (int step = 0; step < 10; step++) {
      List<Start> starts = starts(board);
      if (starts.isEmpty()) {
        out.line(new Boards.Line().str("stage", "via").num("step", step).str("skip", "no starts").end());
        break;
      }
      Start s = drawStart(starts, random);
      int[] dir = DIRS45[random.nextInt(DIRS45.length)];
      int unit = board.getMinTraceHalfWidth();
      int dist = random.nextInt(21) * unit;
      IntPoint location = new IntPoint(s.point.x + dir[0] * dist, s.point.y + dir[1] * dist);
      ViaInfo via = firstVia(board, s.net);
      Boards.Line line =
          new Boards.Line()
              .str("stage", "via")
              .num("step", step)
              .num("net", s.net)
              .str("location", point(location));
      if (via == null) {
        out.line(line.str("skip", "no via").end());
        continue;
      }
      int clearanceClass = board.rules.nets.get(s.net).getNetClass().getTraceClearanceClass();
      int tidy = step % 2 == 0 ? Integer.MAX_VALUE : 0;
      board.generateSnapshot();
      boolean ok =
          board.forcedVia(
              via, location, new int[] {s.net}, clearanceClass, pens(board, s.net), 20, 8, tidy,
              500, 0);
      if (!ok) {
        board.undo(null);
      }
      out.line(state(line.str("via", via.getName()).num("tidy", tidy).bool("ok", ok), board).end());
      if (step % 10 == 9) {
        dump(out, board);
      }
    }
  }

  static void move(Boards.Out out, RoutingBoard board, Random random) {
    for (int step = 0; step < 10; step++) {
      List<Via> vias = new ArrayList<>(board.getVias());
      vias.sort((a, b) -> Integer.compare(b.getId(), a.getId()));
      if (vias.isEmpty()) {
        out.line(new Boards.Line().str("stage", "move").num("step", step).str("skip", "no vias").end());
        break;
      }
      Via via = vias.get(random.nextInt(vias.size()));
      int[] dir = DIRS45[random.nextInt(DIRS45.length)];
      int unit = board.getMinTraceHalfWidth();
      int dist = (1 + random.nextInt(10)) * unit;
      IntVector vector = new IntVector(dir[0] * dist, dir[1] * dist);
      int tidy = step % 2 == 0 ? Integer.MAX_VALUE : 100;
      board.generateSnapshot();
      boolean ok = board.moveDrillItem(via, vector, 20, 8, tidy, 500, 0);
      if (!ok) {
        board.undo(null);
      }
      out.line(
          state(
                  new Boards.Line()
                      .str("stage", "move")
                      .num("step", step)
                      .num("via", via.getId())
                      .str("vector", vector.x + "," + vector.y)
                      .num("tidy", tidy)
                      .bool("ok", ok),
                  board)
              .end());
    }
    dump(out, board);
  }

  static void attach(Boards.Out out, RoutingBoard board) {
    List<Item> items = new ArrayList<>(board.getItems());
    items.sort((a, b) -> Integer.compare(b.getId(), a.getId()));
    for (Item item : items) {
      if (!(item instanceof Pin pin)
          || pin.firstLayer() != pin.lastLayer()
          || pin.netCount() != 1
          || pin.getNetNumber(0) <= 0
          || !(pin.getCenter() instanceof IntPoint center)) {
        continue;
      }
      int net = pin.getNetNumber(0);
      ViaInfo via = firstVia(board, net);
      if (via == null) {
        continue;
      }
      int clearanceClass = board.rules.nets.get(net).getNetClass().getTraceClearanceClass();
      for (boolean attachAllowed : new boolean[] {false, true}) {
        ViaInfo copy =
            new ViaInfo(
                via.getName(), via.getPadstack(), via.getClearanceClassIndex(), attachAllowed,
                board.rules);
        board.generateSnapshot();
        boolean ok =
            board.forcedVia(
                copy, center, new int[] {net}, clearanceClass, pens(board, net), 20, 8,
                Integer.MAX_VALUE, 500, 0);
        board.undo(null);
        out.line(
            state(
                    new Boards.Line()
                        .str("stage", "attach")
                        .num("pin", pin.getId())
                        .num("net", net)
                        .str("via", via.getName())
                        .bool("attach", attachAllowed)
                        .bool("ok", ok),
                    board)
                .end());
      }
      return;
    }
    out.line(new Boards.Line().str("stage", "attach").str("skip", "no smd pin").end());
  }

  // ---- the canonical item dump (tests/support/item_dump.rs) ----

  static String point(Point p) {
    if (p instanceof IntPoint ip) {
      return ip.x + "," + ip.y;
    }
    FloatPoint f = p.toFloat();
    return "f" + Boards.bits(f.x) + "," + Boards.bits(f.y);
  }

  static String corners(Polyline polyline) {
    StringBuilder text = new StringBuilder();
    for (int i = 0; i < polyline.cornerCount(); i++) {
      text.append(i > 0 ? " " : "").append(point(polyline.corner(i)));
    }
    return text.toString();
  }

  static String nets(Item item) {
    StringBuilder text = new StringBuilder();
    for (int i = 0; i < item.netCount(); i++) {
      text.append(i > 0 ? "," : "").append(item.getNetNumber(i));
    }
    return text.toString();
  }

  /** Traces and vias, highest id first, then maxGeneratedId and the item count. */
  static void dump(Boards.Out out, RoutingBoard board) {
    List<Item> items = new ArrayList<>(board.getItems());
    items.sort((a, b) -> Integer.compare(b.getId(), a.getId()));
    for (Item item : items) {
      if (item instanceof PolylineTrace trace) {
        out.line(
            new Boards.Line()
                .num("id", trace.getId())
                .str("kind", "trace")
                .str("nets", nets(trace))
                .num("layer", trace.getLayer())
                .num("halfWidth", trace.getHalfWidth())
                .num("class", trace.clearanceClassIndex())
                .num("fixed", trace.getFixedState().ordinal())
                .str("corners", corners(trace.polyline()))
                .end());
      } else if (item instanceof Via via) {
        out.line(
            new Boards.Line()
                .num("id", via.getId())
                .str("kind", "via")
                .str("nets", nets(via))
                .str("padstack", via.getPadstack().name)
                .str("center", point(via.getCenter()))
                .num("fixed", via.getFixedState().ordinal())
                .end());
      }
    }
    out.line(
        new Boards.Line()
            .num("maxGeneratedId", board.communication.idGenerator.maxGeneratedId())
            .num("items", board.getItems().size())
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
