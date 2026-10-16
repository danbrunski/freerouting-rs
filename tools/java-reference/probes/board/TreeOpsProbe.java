import app.freerouting.Freerouting;
import app.freerouting.board.facade.RoutingBoard;
import app.freerouting.board.model.items.Item;
import app.freerouting.board.model.items.ObstacleArea;
import app.freerouting.board.model.items.Pin;
import app.freerouting.geometry.planar.TileShape;
import app.freerouting.board.searchtree.SearchTreeManager;
import app.freerouting.board.searchtree.ShapeSearchTree;
import app.freerouting.board.trace.PolylineTrace;
import app.freerouting.board.model.items.Trace;
import app.freerouting.autoroute.expansion.CompleteFreeSpaceExpansionRoom;
import app.freerouting.core.RoutingJob;
import app.freerouting.datastructures.ShapeTree;
import app.freerouting.geometry.planar.IntBox;
import app.freerouting.geometry.planar.IntOctagon;
import app.freerouting.geometry.planar.IntPoint;
import app.freerouting.geometry.planar.Line;
import app.freerouting.geometry.planar.Point;
import app.freerouting.geometry.planar.Polyline;
import app.freerouting.geometry.planar.RegularTileShape;
import app.freerouting.io.specctra.SesReader;
import app.freerouting.management.BoardLoader;
import app.freerouting.rules.NetClasses;
import app.freerouting.rules.ViaInfo;
import app.freerouting.rules.ViaRule;
import app.freerouting.board.model.structure.FixedState;
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
import java.util.Collection;
import java.util.Comparator;
import java.util.Deque;
import java.util.List;
import java.util.Random;
import java.util.Set;
import java.util.TreeSet;
import java.util.UUID;

/**
 * Board probe {@code tree-ops} (B02.4, B02.5): the search trees after Java's tree-changing board
 * operations, for decision D5. Runs on the rows with a session (exits 3 on the others): load,
 * create the autoroute trees (as {@code TreeProbe}), import the session, then the stages below,
 * one after the other on the same board. Each stage writes a line with {@code maxGeneratedId} and
 * the item count, then one line per tree as {@code TreeProbe} writes it.
 *
 * <ol>
 *   <li>{@code insert-via}: 20 vias of the default via rule's first via, each at a seeded corner
 *       ({@code Random(2)}) of a seeded trace of the current traces, through {@code insertVia};
 *   <li>{@code remove}: the 30 highest-id traces unfixed and removed ({@code removeItems});
 *   <li>{@code undo-redo}: {@code generateSnapshot}, the {@code remove} step, {@code undo(null)},
 *       {@code redo(null)};
 * </ol>
 *
 * On {@link #BIG} only: {@code compensation} ({@code setClearanceCompensationUsed(true)}, then
 * {@code false}), {@code plane-obstacle} ({@code changePlaneAsObstacle(true)}), {@code
 * make-conductive} (the first {@code ObstacleArea} in item-list order made conductive for net 1).
 * On {@link #CHANGE} only, on a second board loaded the same way: {@code change}, 50 seeded ({@code Random(4)}) {@code
 * PolylineTrace.change} calls on traces with at least 4 corners, each with a new polyline that
 * reuses the old {@code Line} objects except at one or two indexes, where it puts a translated
 * line; the line logs Java's reference-compared prefix {@code p} and suffix {@code s}. On rows
 * with a pin of more than one net: {@code tie-pin} (see {@link #tiePins}).
 *
 * <pre>
 * tools/java-reference/probe.sh &lt;pin&gt; tools/java-reference/probes/board/TreeOpsProbe.java \
 *     board.dsn board.ses out.jsonl
 * </pre>
 */
public class TreeOpsProbe {
  static final Set<String> BIG =
      Set.of("Issue026-J2_reference", "Issue145-smoothieboard", "Issue413-test");
  static final Set<String> CHANGE = Set.of("Issue026-J2_reference", "Issue413-test");
  /** TreeProbe's walk mode, unused here: {@link #hash} is a copy of TreeProbe's. */
  static String walkStage;
  static String walkTree;
  static List<String> walkOut;

  public static void main(String[] args) throws Exception {
    if (args[1].equals("-")) {
      System.exit(3);
    }
    String name = Boards.board(args[0]);
    Boards.Out out = new Boards.Out("tree-ops", args[0]);
    try {
      RoutingBoard board = Boards.load(args[0]);
      autorouteTrees(board);
      Boards.importSession(board, args[1]);
      insertVias(out, board);
      removeTraces(board);
      hashAll(out, "remove", "remove", board);
      board.generateSnapshot();
      removeTraces(board);
      hashAll(out, "undo-redo", "remove", board);
      board.undo(null);
      hashAll(out, "undo-redo", "undo", board);
      board.redo(null);
      hashAll(out, "undo-redo", "redo", board);
      if (BIG.contains(name)) {
        board.searchTreeManager.setClearanceCompensationUsed(true);
        hashAll(out, "compensation", "on", board);
        board.searchTreeManager.setClearanceCompensationUsed(false);
        hashAll(out, "compensation", "off", board);
        autorouteTrees(board);
        hashAll(out, "compensation", "autoroute", board);
        board.changePlaneAsObstacle(true);
        hashAll(out, "plane-obstacle", "plane-obstacle", board);
        makeConductive(out, board);
      }
      if (CHANGE.contains(name)) {
        RoutingBoard changed = Boards.load(args[0]);
        autorouteTrees(changed);
        Boards.importSession(changed, args[1]);
        changeTraces(out, changed);
      }
      tiePins(out, args);
    } catch (Throwable t) {
      out.exception(t);
    }
    out.write(args[2]);
  }

  static void autorouteTrees(RoutingBoard board) {
    TreeSet<Integer> classes = new TreeSet<>();
    NetClasses netClasses = board.rules.netClasses;
    for (int i = 0; i < netClasses.count(); i++) {
      classes.add(netClasses.get(i).getTraceClearanceClass());
    }
    for (int c : classes) {
      board.searchTreeManager.getAutorouteTree(c);
    }
  }

  /** The traces in item-list order. */
  static List<PolylineTrace> traces(RoutingBoard board) {
    List<PolylineTrace> result = new ArrayList<>();
    for (Trace trace : board.getTraces()) {
      if (trace instanceof PolylineTrace polylineTrace) {
        result.add(polylineTrace);
      }
    }
    return result;
  }

  static void insertVias(Boards.Out out, RoutingBoard board) throws Exception {
    ViaRule rule = board.rules.getDefaultViaRule();
    Random random = new Random(2);
    if (rule != null && rule.viaCount() > 0) {
      ViaInfo info = rule.getVia(0);
      for (int k = 0; k < 20; k++) {
        List<PolylineTrace> traces = traces(board);
        if (traces.isEmpty()) {
          break;
        }
        PolylineTrace trace = traces.get(random.nextInt(traces.size()));
        Point corner = trace.polyline().corner(random.nextInt(trace.cornerCount()));
        boolean inserted = false;
        if (corner instanceof IntPoint) {
          board.insertVia(
              info.getPadstack(),
              corner,
              trace.netNumbers,
              info.getClearanceClassIndex(),
              FixedState.UNFIXED,
              info.attachSmdAllowed());
          inserted = true;
        }
        out.line(
            new Boards.Line()
                .str("stage", "insert-via")
                .num("via", k)
                .num("trace", trace.getId())
                .bool("inserted", inserted)
                .end());
      }
    }
    hashAll(out, "insert-via", "insert-via", board);
  }

  /** Unfixes and removes the 30 highest-id traces. */
  static void removeTraces(RoutingBoard board) {
    List<PolylineTrace> traces = traces(board);
    traces.sort(Comparator.comparingInt(Item::getId).reversed());
    List<Item> remove = new ArrayList<>(traces.subList(0, Math.min(30, traces.size())));
    for (Item item : remove) {
      item.unfix();
    }
    board.removeItems(remove);
  }

  static void makeConductive(Boards.Out out, RoutingBoard board) throws Exception {
    ObstacleArea area = null;
    for (Item item : board.getItems()) {
      if (item.getClass() == ObstacleArea.class) {
        area = (ObstacleArea) item;
        break;
      }
    }
    out.line(
        new Boards.Line()
            .str("stage", "make-conductive")
            .num("area", area == null ? -1 : area.getId())
            .end());
    if (area != null) {
      board.makeConductive(area, 1);
    }
    hashAll(out, "make-conductive", "make-conductive", board);
  }

  static void changeTraces(Boards.Out out, RoutingBoard board) throws Exception {
    Random random = new Random(4);
    for (int k = 0; k < 50; k++) {
      List<PolylineTrace> candidates = new ArrayList<>();
      for (PolylineTrace trace : traces(board)) {
        if (trace.cornerCount() >= 4) {
          candidates.add(trace);
        }
      }
      if (candidates.isEmpty()) {
        break;
      }
      PolylineTrace trace = candidates.get(random.nextInt(candidates.size()));
      Line[] old = trace.polyline().lines;
      Line[] lines = old.clone();
      int count = 1 + random.nextInt(2);
      StringBuilder indexes = new StringBuilder();
      for (int c = 0; c < count; c++) {
        int index = 1 + random.nextInt(old.length - 2);
        int dist = (random.nextInt(4) + 1) * 250;
        if (old[index].a instanceof IntPoint && old[index].b instanceof IntPoint) {
          lines[index] = old[index].translate(dist);
          indexes.append(indexes.length() > 0 ? "," : "").append(index).append(':').append(dist);
        }
      }
      Polyline polyline = new Polyline(lines);
      int lastIndex = Math.min(polyline.lines.length, old.length);
      int p = lastIndex;
      for (int i = 0; i < lastIndex; i++) {
        if (polyline.lines[i] != old[i]) {
          p = i;
          break;
        }
      }
      int s = lastIndex;
      for (int i = 1; i <= lastIndex; i++) {
        if (polyline.lines[polyline.lines.length - i] != old[old.length - i]) {
          s = i - 1;
          break;
        }
      }
      long linesHash = Boards.FNV_OFFSET;
      for (Line line : polyline.lines) {
        linesHash = Boards.fnv(linesHash, point(line.a) + " " + point(line.b) + "\n");
      }
      out.line(
          new Boards.Line()
              .str("stage", "change")
              .num("k", k)
              .num("trace", trace.getId())
              .str("indexes", indexes.toString())
              .num("p", p)
              .num("s", s)
              .num("lines", polyline.lines.length)
              .str("linesHash", Boards.hex(linesHash))
              .end());
      if (polyline.lines.length >= 3) {
        trace.change(polyline);
      }
      hashAll(out, "change", "change", board);
    }
  }

  /** The pins of more than one net, in item-list order. */
  static List<Pin> tiePinsOf(RoutingBoard board) {
    List<Pin> result = new ArrayList<>();
    for (Item item : board.getItems()) {
      if (item instanceof Pin pin && pin.netCount() > 1) {
        result.add(pin);
      }
    }
    return result;
  }

  /**
   * Stage {@code tie-pin} (B02.6), on a board loaded like the first one and only if it has a pin
   * of more than one net: per autoroute tree (classes ascending), {@code reduceTraceShapeAtTiePin}
   * for every such pin and every trace ending at its centre, logging the trace's changed tree shape
   * (dimension and bounding octagon), then the tree.
   */
  static void tiePins(Boards.Out out, String[] args) throws Exception {
    RoutingBoard board = Boards.load(args[0]);
    if (tiePinsOf(board).isEmpty()) {
      return;
    }
    autorouteTrees(board);
    Boards.importSession(board, args[1]);
    TreeSet<Integer> classes = new TreeSet<>();
    NetClasses netClasses = board.rules.netClasses;
    for (int i = 0; i < netClasses.count(); i++) {
      classes.add(netClasses.get(i).getTraceClearanceClass());
    }
    for (int c : classes) {
      ShapeSearchTree tree = board.searchTreeManager.getAutorouteTree(c);
      for (Pin pin : tiePinsOf(board)) {
        for (PolylineTrace trace : traces(board)) {
          int shapeNo;
          if (trace.firstCorner().equals(pin.getCenter())) {
            shapeNo = 0;
          } else if (trace.lastCorner().equals(pin.getCenter())) {
            shapeNo = trace.cornerCount() - 2;
          } else {
            continue;
          }
          tree.reduceTraceShapeAtTiePin(pin, trace);
          TileShape shape = trace.getTreeShape(tree, shapeNo);
          StringBuilder bounds = new StringBuilder();
          bounds(bounds, shape.boundingOctagon());
          out.line(
              new Boards.Line()
                  .str("stage", "tie-pin")
                  .str("tree", tree.key)
                  .num("pin", pin.getId())
                  .num("trace", trace.getId())
                  .num("shapeNo", shapeNo)
                  .num("dimension", shape.dimension())
                  .str("bounds", bounds.toString())
                  .end());
        }
      }
      hash(out, "tie-pin", tree);
    }
  }

  static String point(Point point) {
    return point instanceof IntPoint p ? p.x + " " + p.y : "? " + point;
  }

  @SuppressWarnings("unchecked")
  static List<ShapeSearchTree> trees(RoutingBoard board) throws Exception {
    Field field = SearchTreeManager.class.getDeclaredField("compensatedSearchTrees");
    field.setAccessible(true);
    return new ArrayList<>((Collection<ShapeSearchTree>) field.get(board.searchTreeManager));
  }

  static void hashAll(Boards.Out out, String stage, String step, RoutingBoard board)
      throws Exception {
    out.line(
        new Boards.Line()
            .str("stage", stage)
            .str("step", step)
            .num("maxGeneratedId", board.communication.idGenerator.maxGeneratedId())
            .num("items", board.getItems().size())
            .end());
    for (ShapeSearchTree tree : trees(board)) {
      hash(out, stage, tree);
    }
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

  /** Writes one line for {@code tree}: counts, depth, walk hash and content digest. */
  static void hash(Boards.Out out, String stage, ShapeSearchTree tree) throws Exception {
    boolean walk = walkOut != null && stage.equals(walkStage) && tree.key.equals(walkTree);
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
      if (walk) {
        walkOut.add(line.toString());
      }
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
