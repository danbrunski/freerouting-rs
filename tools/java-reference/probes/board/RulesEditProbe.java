import app.freerouting.Freerouting;
import app.freerouting.autoroute.expansion.CompleteFreeSpaceExpansionRoom;
import app.freerouting.board.facade.RoutingBoard;
import app.freerouting.board.model.items.Item;
import app.freerouting.board.model.items.ObstacleArea;
import app.freerouting.board.model.items.Pin;
import app.freerouting.board.model.items.Trace;
import app.freerouting.board.model.structure.FixedState;
import app.freerouting.board.searchtree.SearchTreeManager;
import app.freerouting.board.searchtree.ShapeSearchTree;
import app.freerouting.board.trace.PolylineTrace;
import app.freerouting.core.RoutingJob;
import app.freerouting.datastructures.ShapeTree;
import app.freerouting.geometry.planar.IntBox;
import app.freerouting.geometry.planar.IntOctagon;
import app.freerouting.geometry.planar.IntPoint;
import app.freerouting.geometry.planar.Line;
import app.freerouting.geometry.planar.Point;
import app.freerouting.geometry.planar.Polyline;
import app.freerouting.geometry.planar.RegularTileShape;
import app.freerouting.geometry.planar.TileShape;
import app.freerouting.io.specctra.SesReader;
import app.freerouting.management.BoardLoader;
import app.freerouting.rules.NetClasses;
import app.freerouting.rules.ViaInfo;
import app.freerouting.rules.ViaRule;
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
import java.util.LinkedHashSet;
import java.util.List;
import java.util.Random;
import java.util.Set;
import java.util.TreeSet;
import java.util.UUID;
import app.freerouting.rules.ClearanceMatrix;

/**
 * Board probe {@code rules-edit} (B08.4): the search trees after the calls the clearance matrix
 * window makes (see {@code rules/edit.rs} in the port for the call list). Stages, each followed
 * by the tree lines of {@code tree-ops} (counts, depth, walk hash, content digest per tree):
 *
 * <ol>
 *   <li>{@code load}: the board as loaded and the session imported, with an autoroute tree for
 *       every net class's trace clearance class;
 *   <li>{@code value-changed}: {@code ClearanceMatrix.setValue} of (smd, smd) and back to 3000 on
 *       all layers, then {@code searchTreeManager.clearanceValueChanged()};
 *   <li>{@code class-added}: {@code appendClass("extra")}, then {@code
 *       setClearanceCompensationUsed(false)};
 *   <li>{@code assigned}: every 10th item of clearance class 1 (item-list order) gets {@code
 *       changeClearanceClassIndex(extra)}, then {@code getAutorouteTree(extra)};
 *   <li>{@code pruned}: {@code pruneClearanceMatrix} as the window loops, confirming only
 *       "extra": {@code rules.changeClearanceClassIndex(i, j, items)}, {@code
 *       rules.removeClearanceClass(i, items)}, {@code searchTreeManager.clearanceClassRemoved(i)}.
 * </ol>
 *
 * Runs on the boards in {@link #BOARDS} with their session and exits 3 on the others.
 *
 * <pre>
 * tools/java-reference/probe.sh &lt;pin&gt; tools/java-reference/probes/board/RulesEditProbe.java \
 *     board.dsn board.ses out.jsonl
 * </pre>
 */
public class RulesEditProbe {
  static final Set<String> BOARDS = Set.of("Issue026-J2_reference", "Issue145-smoothieboard");
  static final int VALUE = 3000;
  static String walkStage;
  static String walkTree;
  static List<String> walkOut;

  public static void main(String[] args) throws Exception {
    if (!BOARDS.contains(Boards.board(args[0])) || args[1].equals("-")) {
      System.exit(3);
    }
    Boards.Out out = new Boards.Out("rules-edit", args[0]);
    try {
      RoutingBoard board = Boards.load(args[0]);
      Boards.importSession(board, args[1]);
      autorouteTrees(board);
      hashAll(out, "load", "load", board);

      ClearanceMatrix matrix = board.rules.clearanceMatrix;
      int smd = matrix.getNo("smd");
      matrix.setValue(smd, smd, VALUE);
      matrix.setValue(smd, smd, VALUE);
      board.searchTreeManager.clearanceValueChanged();
      hashAll(out, "value-changed", "smd-smd", board);

      int extra = matrix.getClassCount();
      matrix.appendClass("extra");
      board.searchTreeManager.setClearanceCompensationUsed(false);
      hashAll(out, "class-added", "extra", board);

      int count = 0;
      for (Item item : board.getItems()) {
        if (item.clearanceClassIndex() == 1) {
          if (count++ % 10 == 0) {
            item.changeClearanceClassIndex(extra);
          }
        }
      }
      board.searchTreeManager.getAutorouteTree(extra);
      hashAll(out, "assigned", "moved-" + ((count + 9) / 10), board);

      for (int i = matrix.getClassCount() - 1; i >= 2; i--) {
        for (int j = matrix.getClassCount() - 1; j >= 0; j--) {
          if (i == j) {
            continue;
          }
          if (matrix.isEqual(i, j)) {
            if (matrix.getName(i).equals("extra")) {
              Collection<Item> boardItems = board.getItems();
              board.rules.changeClearanceClassIndex(i, j, boardItems);
              if (!board.rules.removeClearanceClass(i, boardItems)) {
                throw new IllegalStateException("removeClearanceClass failed");
              }
              board.searchTreeManager.clearanceClassRemoved(i);
            }
            break;
          }
        }
      }
      hashAll(out, "pruned", "classes-" + matrix.getClassCount(), board);
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
