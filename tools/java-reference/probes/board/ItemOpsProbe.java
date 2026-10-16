import app.freerouting.Freerouting;
import app.freerouting.autoroute.expansion.CompleteFreeSpaceExpansionRoom;
import app.freerouting.board.facade.RoutingBoard;
import app.freerouting.board.model.items.ComponentOutline;
import app.freerouting.board.model.items.DrillItem;
import app.freerouting.board.model.items.Item;
import app.freerouting.board.model.items.ObstacleArea;
import app.freerouting.board.model.items.Via;
import app.freerouting.board.model.structure.BoardOutline;
import app.freerouting.board.searchtree.ShapeSearchTree;
import app.freerouting.board.trace.PolylineTrace;
import app.freerouting.core.RoutingJob;
import app.freerouting.datastructures.ShapeTree;
import app.freerouting.geometry.planar.FloatPoint;
import app.freerouting.geometry.planar.IntBox;
import app.freerouting.geometry.planar.IntOctagon;
import app.freerouting.geometry.planar.IntPoint;
import app.freerouting.geometry.planar.IntVector;
import app.freerouting.geometry.planar.Point;
import app.freerouting.geometry.planar.Polyline;
import app.freerouting.geometry.planar.RegularTileShape;
import app.freerouting.io.specctra.SesReader;
import app.freerouting.management.BoardLoader;
import app.freerouting.settings.GlobalSettings;
import java.io.FileInputStream;
import java.io.InputStream;
import java.lang.reflect.Field;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayDeque;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.Deque;
import java.util.List;
import java.util.Random;
import java.util.TreeSet;
import java.util.UUID;

/**
 * Board probe {@code item-ops} (B05.1, B05.6): item moves and {@code connectToTrace}. Runs on
 * the boards below with their sessions and exits 3 on the others.
 *
 * <ul>
 *   <li>{@code move} (Issue508-DAC2020_bm05): {@code generateSnapshot}, then the 20 highest-id
 *       vias moved by (+500, +250) with {@code DrillItem.moveBy}, one after the other. After each
 *       move a line with the via, {@code maxGeneratedId}, the item count and FNV-1a 64 over one
 *       line per item in item-list order ({@link #itemText}), then the default tree's
 *       fingerprint (as {@code TreeOpsProbe} writes it). Then {@code undo(null)} and whether
 *       {@code getHash} equals its value before the snapshot and right after it, and whether the
 *       item hash equals its value before the moves.
 *   <li>{@code connect} (Issue026-J2_reference): every trace unfixed, then 50 seeded ({@code
 *       Random(5)}) pairs: a trace of the current traces, one of its corners and an offset of at
 *       most 5 trace widths on each axis; {@code connectToTrace(point, trace, halfWidth,
 *       clearanceClass)} with the trace's half width and class. A line with the result, the ids
 *       inserted and removed (item-list order), {@code maxGeneratedId} and the item hash.
 * </ul>
 *
 * <pre>
 * tools/java-reference/probe.sh &lt;pin&gt; tools/java-reference/probes/board/ItemOpsProbe.java \
 *     board.dsn board.ses out.jsonl
 * </pre>
 */
public class ItemOpsProbe {
  static final String MOVE = "Issue508-DAC2020_bm05";
  static final String CONNECT = "Issue026-J2_reference";

  public static void main(String[] args) throws Exception {
    String name = Boards.board(args[0]);
    if (args[1].equals("-") || !(name.equals(MOVE) || name.equals(CONNECT))) {
      System.exit(3);
    }
    Boards.Out out = new Boards.Out("item-ops", args[0]);
    try {
      RoutingBoard board = Boards.load(args[0]);
      Boards.importSession(board, args[1]);
      if (name.equals(MOVE)) {
        moves(out, board);
      }
      if (name.equals(CONNECT)) {
        connects(out, board);
        RoutingBoard fresh = Boards.load(args[0]);
        Boards.importSession(fresh, args[1]);
        transforms(out, fresh);
      }
    } catch (Throwable t) {
      out.exception(t);
    }
    out.write(args[2]);
  }

  static void moves(Boards.Out out, RoutingBoard board) throws Exception {
    String before = board.getHash();
    long itemsBefore = itemsHash(board);
    board.generateSnapshot();
    String snapshot = board.getHash();
    List<Via> vias = new ArrayList<>();
    for (Item item : board.getItems()) {
      if (item instanceof Via via && vias.size() < 20) {
        vias.add(via);
      }
    }
    for (int k = 0; k < vias.size(); k++) {
      Via via = vias.get(k);
      via.moveBy(new IntVector(500, 250));
      out.line(
          new Boards.Line()
              .str("stage", "move")
              .num("k", k)
              .num("via", via.getId())
              .str("center", point(via.getCenter()))
              .num("maxGeneratedId", board.communication.idGenerator.maxGeneratedId())
              .num("items", board.getItems().size())
              .str("itemsHash", Boards.hex(itemsHash(board)))
              .end());
      hash(out, "move", board.searchTreeManager.getDefaultTree());
    }
    board.undo(null);
    out.line(
        new Boards.Line()
            .str("stage", "move")
            .str("step", "undo")
            .num("maxGeneratedId", board.communication.idGenerator.maxGeneratedId())
            .num("items", board.getItems().size())
            .bool("hashEqualsBefore", board.getHash().equals(before))
            .bool("hashEqualsSnapshot", board.getHash().equals(snapshot))
            .bool("itemsEqualBefore", itemsHash(board) == itemsBefore)
            .end());
  }

  static void connects(Boards.Out out, RoutingBoard board) throws Exception {
    for (Item item : board.getItems()) {
      if (item instanceof PolylineTrace) {
        item.unfix();
      }
    }
    Random random = new Random(5);
    for (int k = 0; k < 50; k++) {
      List<PolylineTrace> traces = new ArrayList<>();
      for (Item item : board.getItems()) {
        if (item instanceof PolylineTrace trace) {
          traces.add(trace);
        }
      }
      if (traces.isEmpty()) {
        break;
      }
      PolylineTrace trace = traces.get(random.nextInt(traces.size()));
      int corner = random.nextInt(trace.cornerCount());
      int range = 10 * trace.getHalfWidth();
      int dx = random.nextInt(2 * range + 1) - range;
      int dy = random.nextInt(2 * range + 1) - range;
      Point c = trace.polyline().corner(corner);
      Boards.Line line =
          new Boards.Line()
              .str("stage", "connect")
              .num("k", k)
              .num("trace", trace.getId())
              .num("corner", corner);
      if (!(c instanceof IntPoint ic)) {
        out.line(line.str("skip", "rational corner").end());
        continue;
      }
      IntPoint from = new IntPoint(ic.x + dx, ic.y + dy);
      TreeSet<Integer> beforeIds = ids(board);
      boolean result =
          board.connectToTrace(from, trace, trace.getHalfWidth(), trace.clearanceClassIndex());
      TreeSet<Integer> afterIds = ids(board);
      out.line(
          line.str("from", point(from))
              .bool("result", result)
              .str("inserted", difference(afterIds, beforeIds))
              .str("removed", difference(beforeIds, afterIds))
              .num("maxGeneratedId", board.communication.idGenerator.maxGeneratedId())
              .num("items", board.getItems().size())
              .str("itemsHash", Boards.hex(itemsHash(board)))
              .end());
    }
  }

  static final IntPoint POLE = new IntPoint(1200000, -850000);

  /**
   * Stage {@code transform}: the first trace, via and component pin in item-list order, each
   * removed from the trees, transformed with the item's own method and inserted again, for
   * {@code turn90Degree(1)}, {@code changePlacementSide}, {@code rotateApprox(30)} and {@code
   * rotateApprox(45.5)} around {@link #POLE}, one after the other. The pin's component gets the
   * same transform first ({@code Components.turn90Degree}, {@code changeSide}, {@code rotate}).
   */
  static void transforms(Boards.Out out, RoutingBoard board) throws Exception {
    List<Item> items = new ArrayList<>();
    for (Class<?> kind : List.of(PolylineTrace.class, Via.class, app.freerouting.board.model.items.Pin.class)) {
      for (Item item : board.getItems()) {
        if (kind.isInstance(item) && (kind != app.freerouting.board.model.items.Pin.class || item.getComponentId() > 0)) {
          items.add(item);
          break;
        }
      }
    }
    String[] ops = {"turn90", "side", "rotate30", "rotate45.5"};
    for (String op : ops) {
      for (Item item : items) {
        board.searchTreeManager.remove(item);
        if (item instanceof app.freerouting.board.model.items.Pin) {
          int c = item.getComponentId();
          switch (op) {
            case "turn90" -> board.components.turn90Degree(c, 1, POLE);
            case "side" -> board.components.changeSide(c, POLE);
            case "rotate30" -> board.components.rotate(c, 30, POLE);
            default -> board.components.rotate(c, 45.5, POLE);
          }
        }
        switch (op) {
          case "turn90" -> item.turn90Degree(1, POLE);
          case "side" -> item.changePlacementSide(POLE);
          case "rotate30" -> item.rotateApprox(30, POLE.toFloat());
          default -> item.rotateApprox(45.5, POLE.toFloat());
        }
        board.searchTreeManager.insert(item);
        out.line(
            new Boards.Line()
                .str("stage", "transform")
                .str("op", op)
                .str("item", itemText(item).trim())
                .end());
      }
    }
  }

  static TreeSet<Integer> ids(RoutingBoard board) {
    TreeSet<Integer> result = new TreeSet<>();
    for (Item item : board.getItems()) {
      result.add(item.getId());
    }
    return result;
  }

  /** The ids in {@code a} but not {@code b}, highest first, comma separated. */
  static String difference(TreeSet<Integer> a, TreeSet<Integer> b) {
    StringBuilder text = new StringBuilder();
    for (int id : a.descendingSet()) {
      if (!b.contains(id)) {
        text.append(text.length() > 0 ? "," : "").append(id);
      }
    }
    return text.toString();
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
