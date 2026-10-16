import app.freerouting.Freerouting;
import app.freerouting.board.facade.RoutingBoard;
import app.freerouting.board.model.items.ComponentOutline;
import app.freerouting.board.model.items.DrillItem;
import app.freerouting.board.model.items.Item;
import app.freerouting.board.model.items.ObstacleArea;
import app.freerouting.board.model.items.Pin;
import app.freerouting.autoroute.expansion.CompleteFreeSpaceExpansionRoom;
import app.freerouting.board.model.structure.BoardOutline;
import app.freerouting.board.searchtree.ShapeSearchTree;
import app.freerouting.datastructures.ShapeTree;
import app.freerouting.geometry.planar.IntOctagon;
import app.freerouting.geometry.planar.RegularTileShape;
import java.lang.reflect.Field;
import java.util.ArrayDeque;
import java.util.Arrays;
import java.util.Deque;
import app.freerouting.board.model.structure.AngleRestriction;
import app.freerouting.board.trace.PolylineTrace;
import app.freerouting.core.RoutingJob;
import app.freerouting.geometry.planar.FloatPoint;
import app.freerouting.geometry.planar.IntBox;
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
import java.util.ArrayList;
import java.util.Collection;
import java.util.List;
import java.util.Set;
import java.util.UUID;

/**
 * Board probe {@code pin-connection} (B05.5): the three pin-connection fixes of {@code
 * PolylineTrace.pullTight}. On {@link #BOARDS} with their sessions (exits 3 elsewhere): {@code
 * rules.setPinEdgeToTurnDist(2 * getMinTraceHalfWidth())}, then two passes: for every trace in item-list
 * order (the ids taken before the first fix), while it is on the board, and for its start, then
 * its end, if a pin is among that end's contacts: {@code checkConnectionToPin}, then {@code
 * correctConnectionToPin(atStart, rules.getTraceAngleRestriction())}, then {@code
 * swapConnectionToPin}, each only while the trace is still on the board ({@code "gone"}
 * otherwise). A line per trace end with the three results, the trace's corners afterwards,
 * {@code maxGeneratedId}, the item count and the item hash ({@code ItemOpsProbe.itemText}).  After a
 * correction that changed the trace, the default tree's fingerprint (as {@code TreeOpsProbe}
 * writes it), which checks the line reuse of the {@code change} call. The second pass sees the shove-fixed exit stubs the first one inserted, which {@code
 * swapConnectionToPin} looks for.
 *
 * <pre>
 * tools/java-reference/probe.sh &lt;pin&gt; tools/java-reference/probes/board/PinConnectionProbe.java \
 *     board.dsn board.ses out.jsonl
 * </pre>
 */
public class PinConnectionProbe {
  static final Set<String> BOARDS =
      Set.of("Issue026-J2_reference", "Issue508-DAC2020_bm08", "Issue145-smoothieboard");

  public static void main(String[] args) throws Exception {
    if (args[1].equals("-") || !BOARDS.contains(Boards.board(args[0]))) {
      System.exit(3);
    }
    Boards.Out out = new Boards.Out("pin-connection", args[0]);
    try {
      RoutingBoard board = Boards.load(args[0]);
      Boards.importSession(board, args[1]);
      fixes(out, board);
    } catch (Throwable t) {
      out.exception(t);
    }
    out.write(args[2]);
  }

  static boolean pinAt(PolylineTrace trace, boolean atStart) {
    Collection<Item> contacts = atStart ? trace.getStartContacts() : trace.getEndContacts();
    for (Item item : contacts) {
      if (item instanceof Pin) {
        return true;
      }
    }
    return false;
  }

  static String corners(PolylineTrace trace) {
    if (!trace.isOnTheBoard()) {
      return "gone";
    }
    StringBuilder text = new StringBuilder();
    Polyline polyline = trace.polyline();
    for (int i = 0; i < polyline.cornerCount(); i++) {
      text.append(i > 0 ? " " : "").append(point(polyline.corner(i)));
    }
    return text.toString();
  }

  static void fixes(Boards.Out out, RoutingBoard board) throws Exception {
    int dist = 2 * board.getMinTraceHalfWidth();
    board.rules.setPinEdgeToTurnDist(dist);
    AngleRestriction angle = board.rules.getTraceAngleRestriction();
    out.line(
        new Boards.Line()
            .str("stage", "fixes")
            .num("pinEdgeToTurnDist", dist)
            .str("angle", angle.name())
            .num("maxGeneratedId", board.communication.idGenerator.maxGeneratedId())
            .end());
    for (int pass = 1; pass <= 2; pass++) {
      fixPass(out, board, angle, pass);
    }
  }

  static void fixPass(Boards.Out out, RoutingBoard board, AngleRestriction angle, int pass)
      throws Exception {
    List<PolylineTrace> traces = new ArrayList<>();
    for (Item item : board.getItems()) {
      if (item instanceof PolylineTrace trace) {
        traces.add(trace);
      }
    }
    for (PolylineTrace trace : traces) {
      for (boolean atStart : new boolean[] {true, false}) {
        if (!trace.isOnTheBoard() || !pinAt(trace, atStart)) {
          continue;
        }
        boolean check = trace.checkConnectionToPin(atStart);
        String correct = "gone";
        String swap = "gone";
        if (trace.isOnTheBoard()) {
          correct = Boolean.toString(trace.correctConnectionToPin(atStart, angle));
        }
        if (trace.isOnTheBoard()) {
          swap = Boolean.toString(trace.swapConnectionToPin(atStart));
        }
        out.line(
            new Boards.Line()
                .num("pass", pass)
                .num("trace", trace.getId())
                .bool("atStart", atStart)
                .bool("check", check)
                .str("correct", correct)
                .str("swap", swap)
                .str("corners", corners(trace))
                .num("maxGeneratedId", board.communication.idGenerator.maxGeneratedId())
                .num("items", board.getItems().size())
                .str("itemsHash", Boards.hex(itemsHash(board)))
                .end());
        if (correct.equals("true")) {
          hash(out, "tree", board.searchTreeManager.getDefaultTree());
        }
      }
    }
  }

  static String point(Point p) {
    if (p instanceof IntPoint ip) {
      return ip.x + "," + ip.y;
    }
    FloatPoint f = p.toFloat();
    return "f" + Boards.bits(f.x) + "," + Boards.bits(f.y);
  }

  static String box(IntBox b) {
    return b.ll.x + "," + b.ll.y + "," + b.ur.x + "," + b.ur.y;
  }

  /**
   * One item: id, simple class name, nets, fixed state ordinal, clearance class, then by kind:
   * a drill item's centre, first and last layer and bounding box; a trace's layer, half width
   * and corners; an area's layer and bounding box; an outline's bounding box.
   */
  static String itemText(Item item) {
    StringBuilder text = new StringBuilder();
    text.append(item.getId()).append(' ').append(item.getClass().getSimpleName());
    text.append(" n");
    for (int i = 0; i < item.netCount(); i++) {
      text.append(i > 0 ? "," : "").append(item.getNetNumber(i));
    }
    text.append(" f").append(item.getFixedState().ordinal());
    text.append(" c").append(item.clearanceClassIndex());
    if (item instanceof DrillItem drill) {
      text.append(' ').append(point(drill.getCenter()));
      text.append(" l").append(drill.firstLayer()).append('-').append(drill.lastLayer());
      text.append(' ').append(box(drill.boundingBox()));
    } else if (item instanceof PolylineTrace trace) {
      text.append(" l").append(trace.getLayer()).append(" w").append(trace.getHalfWidth());
      Polyline polyline = trace.polyline();
      for (int i = 0; i < polyline.cornerCount(); i++) {
        text.append(' ').append(point(polyline.corner(i)));
      }
    } else if (item instanceof ObstacleArea area) {
      text.append(" l").append(area.getLayer()).append(' ').append(box(area.boundingBox()));
    } else if (item instanceof ComponentOutline outline) {
      text.append(' ').append(box(outline.boundingBox()));
    } else if (item instanceof BoardOutline outline) {
      text.append(' ').append(box(outline.boundingBox()));
    }
    return text.append('\n').toString();
  }

  static long itemsHash(RoutingBoard board) {
    long hash = Boards.FNV_OFFSET;
    for (Item item : board.getItems()) {
      hash = Boards.fnv(hash, itemText(item));
    }
    return hash;
  }

  static Object get(String className, String fieldName, Object target) throws Exception {
    Field field = Class.forName(className).getDeclaredField(fieldName);
    field.setAccessible(true);
    return field.get(target);
  }

  static void bounds(StringBuilder text, RegularTileShape shape) {
    if (shape instanceof IntOctagon o) {
      text.append("O ")
          .append(o.leftX).append(' ')
          .append(o.bottomY).append(' ')
          .append(o.rightX).append(' ')
          .append(o.topY).append(' ')
          .append(o.upperLeftDiagonalX).append(' ')
          .append(o.lowerRightDiagonalX).append(' ')
          .append(o.lowerLeftDiagonalX).append(' ')
          .append(o.upperRightDiagonalX);
    } else if (shape instanceof IntBox b) {
      text.append("B ")
          .append(b.ll.x).append(' ')
          .append(b.ll.y).append(' ')
          .append(b.ur.x).append(' ')
          .append(b.ur.y);
    } else {
      text.append("? ").append(shape);
    }
  }

  static void object(StringBuilder text, ShapeTree.Leaf leaf) {
    if (leaf.object instanceof CompleteFreeSpaceExpansionRoom room) {
      text.append("R ").append(room.getId());
    } else if (leaf.object instanceof Item item) {
      text.append("I ").append(item.getId());
    } else {
      text.append("? ").append(leaf.object);
    }
    text.append(' ').append(leaf.shapeIndexInObject);
  }

  static final String TREE_NODE = "app.freerouting.datastructures.ShapeTree$TreeNode";

  /** One line for {@code tree}, as {@code TreeOpsProbe.hash} writes it. */
  static void hash(Boards.Out out, String stage, ShapeSearchTree tree) throws Exception {
    long shapeHash = Boards.FNV_OFFSET;
    int nodes = 0;
    int leaves = 0;
    int maxDepth = 0;
    Object root = get("app.freerouting.datastructures.ShapeTree", "root", tree);
    Deque<Object[]> stack = new ArrayDeque<>();
    if (root != null) {
      stack.push(new Object[] {root, 0});
    }
    while (!stack.isEmpty()) {
      Object[] top = stack.pop();
      Object node = top[0];
      int depth = (Integer) top[1];
      nodes++;
      maxDepth = Math.max(maxDepth, depth);
      StringBuilder line = new StringBuilder();
      line.append(depth);
      RegularTileShape shape = (RegularTileShape) get(TREE_NODE, "boundingShape", node);
      if (node instanceof ShapeTree.Leaf leaf) {
        leaves++;
        line.append(" L ");
        bounds(line, shape);
        line.append(' ');
        object(line, leaf);
      } else {
        ShapeTree.InnerNode inner = (ShapeTree.InnerNode) node;
        line.append(" I ");
        bounds(line, shape);
        Object first = ShapeTree.InnerNode.class.getField("firstChild").get(inner);
        Object second = ShapeTree.InnerNode.class.getField("secondChild").get(inner);
        stack.push(new Object[] {second, depth + 1});
        stack.push(new Object[] {first, depth + 1});
      }
      line.append('\n');
      shapeHash = Boards.fnv(shapeHash, line.toString());
    }
    ShapeTree.Leaf[] sorted = tree.toArray();
    Arrays.sort(sorted);
    long contentDigest = Boards.FNV_OFFSET;
    for (ShapeTree.Leaf leaf : sorted) {
      StringBuilder line = new StringBuilder();
      object(line, leaf);
      line.append(' ');
      bounds(line, (RegularTileShape) get(TREE_NODE, "boundingShape", leaf));
      line.append('\n');
      contentDigest = Boards.fnv(contentDigest, line.toString());
    }
    out.line(
        new Boards.Line()
            .str("stage", stage)
            .str("tree", tree.key)
            .num("nodes", nodes)
            .num("leaves", leaves)
            .num("maxDepth", maxDepth)
            .str("shapeHash", Boards.hex(shapeHash))
            .str("contentDigest", Boards.hex(contentDigest))
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
