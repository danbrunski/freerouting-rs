import app.freerouting.Freerouting;
import app.freerouting.autoroute.expansion.CompleteFreeSpaceExpansionRoom;
import app.freerouting.board.facade.RoutingBoard;
import app.freerouting.board.model.items.Item;
import app.freerouting.board.searchtree.SearchTreeManager;
import app.freerouting.board.searchtree.ShapeSearchTree;
import app.freerouting.core.RoutingJob;
import app.freerouting.datastructures.ShapeTree;
import app.freerouting.geometry.planar.IntBox;
import app.freerouting.geometry.planar.IntOctagon;
import app.freerouting.geometry.planar.RegularTileShape;
import app.freerouting.io.specctra.SesReader;
import app.freerouting.management.BoardLoader;
import app.freerouting.rules.NetClasses;
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
import java.util.Deque;
import java.util.List;
import java.util.TreeSet;
import java.util.UUID;

/**
 * Board probe {@code tree} (B02.2): the shape of every search tree, for decision D5. One line per
 * tree and stage with the node and leaf counts, the maximum depth, the FNV-1a 64 of the canonical
 * preorder walk ({@code shapeHash}) and of the sorted leaves ({@code contentDigest}); the walk is
 * the one {@code board/search/fingerprint.rs} documents. Stages: {@code load} (the default tree);
 * {@code autoroute} ({@code getAutorouteTree(c)} for every distinct trace clearance class of the
 * net classes, ascending, each new tree); with a session, {@code ses} (the session imported with
 * those trees present, every tree), {@code deep-copy} (the default tree of {@code deepCopy()} of
 * the {@code ses} board), and on a second board {@code undo} and {@code redo} (from the
 * {@code autoroute} state: {@code generateSnapshot}, import the session, {@code undo(null)}, then
 * {@code redo(null)}; every tree).
 *
 * <pre>
 * tools/java-reference/probe.sh &lt;pin&gt; tools/java-reference/probes/board/TreeProbe.java \
 *     board.dsn board.ses|- out.jsonl [--walk STAGE TREE_KEY]
 * </pre>
 *
 * With {@code --walk}, the probe writes the full walk of that stage's tree (one line per node)
 * to {@code out.jsonl} instead, for a diff against the Rust walk.
 */
public class TreeProbe {
  static String walkStage;
  static String walkTree;
  static List<String> walkOut;

  public static void main(String[] args) throws Exception {
    if (args.length >= 6 && args[3].equals("--walk")) {
      walkStage = args[4];
      walkTree = args[5];
      walkOut = new ArrayList<>();
    }
    Boards.Out out = new Boards.Out("tree", args[0]);
    try {
      RoutingBoard board = Boards.load(args[0]);
      hash(out, "load", board.searchTreeManager.getDefaultTree());
      autorouteTrees(out, board, true);
      if (!args[1].equals("-")) {
        Boards.importSession(board, args[1]);
        hashAll(out, "ses", board);
        RoutingBoard copy = board.deepCopy();
        hash(out, "deep-copy", copy.searchTreeManager.getDefaultTree());

        RoutingBoard second = Boards.load(args[0]);
        autorouteTrees(out, second, false);
        second.generateSnapshot();
        Boards.importSession(second, args[1]);
        second.undo(null);
        hashAll(out, "undo", second);
        second.redo(null);
        hashAll(out, "redo", second);
      }
    } catch (Throwable t) {
      out.exception(t);
    }
    if (walkOut != null) {
      Files.writeString(Path.of(args[2]), String.join("", walkOut), StandardCharsets.UTF_8);
    } else {
      out.write(args[2]);
    }
  }

  /** The distinct trace clearance classes of the net classes, ascending. */
  static TreeSet<Integer> traceClearanceClasses(RoutingBoard board) {
    TreeSet<Integer> classes = new TreeSet<>();
    NetClasses netClasses = board.rules.netClasses;
    for (int i = 0; i < netClasses.count(); i++) {
      classes.add(netClasses.get(i).getTraceClearanceClass());
    }
    return classes;
  }

  static void autorouteTrees(Boards.Out out, RoutingBoard board, boolean log) throws Exception {
    for (int c : traceClearanceClasses(board)) {
      int before = trees(board).size();
      ShapeSearchTree tree = board.searchTreeManager.getAutorouteTree(c);
      if (log && trees(board).size() > before) {
        hash(out, "autoroute", tree);
      }
    }
  }

  @SuppressWarnings("unchecked")
  static List<ShapeSearchTree> trees(RoutingBoard board) throws Exception {
    Field field = SearchTreeManager.class.getDeclaredField("compensatedSearchTrees");
    field.setAccessible(true);
    return new ArrayList<>((Collection<ShapeSearchTree>) field.get(board.searchTreeManager));
  }

  static void hashAll(Boards.Out out, String stage, RoutingBoard board) throws Exception {
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
