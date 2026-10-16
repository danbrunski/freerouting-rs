import app.freerouting.Freerouting;
import app.freerouting.autoroute.expansion.CompleteFreeSpaceExpansionRoom;
import app.freerouting.autoroute.expansion.IncompleteFreeSpaceExpansionRoom;
import app.freerouting.board.facade.RoutingBoard;
import app.freerouting.board.model.structure.AngleRestriction;
import app.freerouting.board.searchtree.ShapeSearchTree;
import app.freerouting.core.RoutingJob;
import app.freerouting.datastructures.ShapeTree;
import app.freerouting.datastructures.WorkMeter;
import app.freerouting.geometry.planar.IntBox;
import app.freerouting.geometry.planar.IntOctagon;
import app.freerouting.geometry.planar.IntPoint;
import app.freerouting.geometry.planar.Line;
import app.freerouting.geometry.planar.Point;
import app.freerouting.geometry.planar.RegularTileShape;
import app.freerouting.geometry.planar.Simplex;
import app.freerouting.geometry.planar.TileShape;
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
import java.util.Map;
import java.util.Random;
import java.util.UUID;

/**
 * Board probe {@code complete-shape} (B03): free-space room completion, {@code
 * ShapeSearchTree.completeShape} and its 45 and 90 degree overrides, on seeded queries. Needs a
 * pin with {@code WorkMeter} ({@code a773a70ec}).
 *
 * <p>Per variant (stage {@code 45}, {@code 90}, {@code plain}) the board is loaded, the trace
 * angle restriction set ({@code FORTYFIVE_DEGREE}, {@code NINETY_DEGREE}, {@code NONE}) and
 * {@code getAutorouteTree(c)} called, with {@code c} the smallest non-zero trace clearance class
 * of the net classes, else 0. Then 500 queries from {@code Random(20261015)}, drawn in this order:
 * layer {@code nextInt(layers)}; {@code x = ll.x + nextInt(width + 1)}, the same for {@code y};
 * half size {@code h = 1 + nextInt(max(1, min(width, height) / 50))}; net {@code
 * nextInt(maxNetNumber + 1)}; {@code nextBoolean()} for a bounded start room. The contained shape
 * is an {@code IntOctagon} (45, plain) or {@code IntBox} (90) of half size {@code h} around the
 * point; a bounded start room is the same kind with half size {@code 20 * h}, else the start shape
 * is {@code null}. One line per query: the inputs, the {@code WorkMeter} units of the call, and the
 * result rooms in order ({@code shape ; layer ; contained shape}), or an exception record.
 *
 * <p>Stage {@code with-rooms} (after stage {@code 45}, same tree and random): the first 100 result
 * rooms are inserted as {@code CompleteFreeSpaceExpansionRoom(shape, layer, id)}, ids 1, 2, ...
 * (the tree is hashed), then 200 more queries, each with {@code ignoreObject} the inserted room
 * {@code nextInt(inserted)} (drawn last) and {@code ignoreShape} its shape intersected with the
 * contained shape. Then the rooms are removed in insertion order and the tree hashed again.
 *
 * <p>Shapes: {@code O} and the eight octagon fields, {@code B llx lly urx ury}, {@code S} and the
 * two points of each line of a simplex, {@code -} for null. Runs on the boards in {@link #VARIANTS}
 * only and exits 3 on the others.
 *
 * <pre>
 * tools/java-reference/probe.sh &lt;pin&gt; tools/java-reference/probes/board/CompleteShapeProbe.java \
 *     board.dsn board.ses out.jsonl
 * </pre>
 */
public class CompleteShapeProbe {
  static final Map<String, List<String>> VARIANTS =
      Map.of(
          "Issue026-J2_reference", List.of("45", "90", "plain"),
          "Issue508-DAC2020_bm08", List.of("45", "90", "plain"),
          "Issue413-test", List.of("45"),
          "Issue145-smoothieboard", List.of("45"),
          "Issue676-ch32v-tx118s", List.of("45"));
  static final long SEED = 20261015L;
  static final int QUERIES = 500;
  static final int ROOM_QUERIES = 200;
  static final int INSERTED_ROOMS = 100;

  public static void main(String[] args) throws Exception {
    List<String> variants = VARIANTS.get(Boards.board(args[0]));
    if (variants == null) {
      System.exit(3);
    }
    Boards.Out out = new Boards.Out("complete-shape", args[0]);
    try {
      for (String variant : variants) {
        variant(out, args[0], variant);
      }
    } catch (Throwable t) {
      out.exception(t);
    }
    out.write(args[2]);
  }

  static void variant(Boards.Out out, String dsn, String variant) throws Exception {
    RoutingBoard board = Boards.load(dsn);
    AngleRestriction angle =
        switch (variant) {
          case "90" -> AngleRestriction.NINETY_DEGREE;
          case "plain" -> AngleRestriction.NONE;
          default -> AngleRestriction.FORTYFIVE_DEGREE;
        };
    board.rules.setTraceAngleRestriction(angle);
    int c = treeClass(board);
    ShapeSearchTree tree = board.searchTreeManager.getAutorouteTree(c);
    out.line(
        new Boards.Line()
            .str("stage", variant)
            .str("tree", tree.key)
            .num("class", c)
            .num("leaves", tree.size())
            .end());
    Sampler sampler = new Sampler(board, variant.equals("90"));
    List<IncompleteFreeSpaceExpansionRoom> results = new ArrayList<>();
    for (int q = 0; q < QUERIES; q++) {
      Query query = sampler.next();
      Boards.Line line = query.line(variant, q);
      complete(out, line, tree, query, null, null, results);
    }
    if (variant.equals("45")) {
      withRooms(out, tree, sampler, results);
    }
  }

  /** The smallest non-zero trace clearance class of the net classes, else 0. */
  static int treeClass(RoutingBoard board) {
    NetClasses netClasses = board.rules.netClasses;
    int result = 0;
    for (int i = 0; i < netClasses.count(); i++) {
      int c = netClasses.get(i).getTraceClearanceClass();
      if (c > 0 && (result == 0 || c < result)) {
        result = c;
      }
    }
    return result;
  }

  static void complete(
      Boards.Out out,
      Boards.Line line,
      ShapeSearchTree tree,
      Query query,
      CompleteFreeSpaceExpansionRoom ignore,
      TileShape ignoreShape,
      List<IncompleteFreeSpaceExpansionRoom> results) {
    WorkMeter meter = WorkMeter.current();
    long before = meter.units();
    Collection<IncompleteFreeSpaceExpansionRoom> rooms;
    try {
      rooms = tree.completeShape(query.room, query.net, ignore, ignoreShape);
    } catch (Throwable t) {
      StackTraceElement[] trace = t.getStackTrace();
      out.line(
          line.num("units", meter.units() - before)
              .str("exception", t.toString())
              .str("at", trace.length > 0 ? trace[0].toString() : "")
              .end());
      return;
    }
    long units = meter.units() - before;
    StringBuilder list = new StringBuilder("[");
    for (IncompleteFreeSpaceExpansionRoom room : rooms) {
      if (list.length() > 1) {
        list.append(',');
      }
      list.append('"');
      shape(list, room.getShape());
      list.append(" ; ").append(room.getLayer()).append(" ; ");
      shape(list, room.getContainedShape());
      list.append('"');
      results.add(room);
    }
    list.append(']');
    out.line(line.num("units", units).raw("rooms", list.toString()).end());
  }

  static void withRooms(
      Boards.Out out,
      ShapeSearchTree tree,
      Sampler sampler,
      List<IncompleteFreeSpaceExpansionRoom> results)
      throws Exception {
    List<CompleteFreeSpaceExpansionRoom> inserted = new ArrayList<>();
    for (int i = 0; i < Math.min(INSERTED_ROOMS, results.size()); i++) {
      IncompleteFreeSpaceExpansionRoom result = results.get(i);
      CompleteFreeSpaceExpansionRoom room =
          new CompleteFreeSpaceExpansionRoom(result.getShape(), result.getLayer(), i + 1);
      tree.insert(room);
      inserted.add(room);
    }
    hash(out, "with-rooms", "inserted", tree, inserted.size());
    List<IncompleteFreeSpaceExpansionRoom> ignored = new ArrayList<>();
    for (int q = 0; q < ROOM_QUERIES; q++) {
      Query query = sampler.next();
      CompleteFreeSpaceExpansionRoom ignore = null;
      TileShape ignoreShape = null;
      if (!inserted.isEmpty()) {
        ignore = inserted.get(sampler.random.nextInt(inserted.size()));
        ignoreShape = ignore.getShape().intersection(query.room.getContainedShape());
      }
      Boards.Line line = query.line("with-rooms", QUERIES + q);
      line.num("ignore", ignore == null ? 0 : ignore.getId());
      StringBuilder text = new StringBuilder();
      shape(text, ignoreShape);
      line.str("ignoreShape", text.toString());
      complete(out, line, tree, query, ignore, ignoreShape, ignored);
    }
    for (CompleteFreeSpaceExpansionRoom room : inserted) {
      room.removeFromTree(tree);
    }
    hash(out, "with-rooms", "removed", tree, inserted.size());
  }

  /** The seeded queries of one variant. */
  static final class Sampler {
    final Random random = new Random(SEED);
    final RoutingBoard board;
    final boolean boxes;
    final IntBox box;
    final int width;
    final int height;
    final int maxNet;

    Sampler(RoutingBoard board, boolean boxes) {
      this.board = board;
      this.boxes = boxes;
      this.box = board.getBoundingBox();
      this.width = box.ur.x - box.ll.x;
      this.height = box.ur.y - box.ll.y;
      this.maxNet = board.rules.nets.maxNetNumber();
    }

    Query next() {
      Query query = new Query();
      query.layer = random.nextInt(board.getLayerCount());
      query.x = box.ll.x + random.nextInt(width + 1);
      query.y = box.ll.y + random.nextInt(height + 1);
      query.h = 1 + random.nextInt(Math.max(1, Math.min(width, height) / 50));
      query.net = random.nextInt(maxNet + 1);
      query.bounded = random.nextBoolean();
      TileShape contained = square(query.x, query.y, query.h);
      TileShape start = query.bounded ? square(query.x, query.y, 20 * query.h) : null;
      query.room = new IncompleteFreeSpaceExpansionRoom(start, query.layer, contained);
      return query;
    }

    TileShape square(int x, int y, int h) {
      IntBox square = new IntBox(x - h, y - h, x + h, y + h);
      return boxes ? square : square.toIntOctagon();
    }
  }

  static final class Query {
    int layer;
    int x;
    int y;
    int h;
    int net;
    boolean bounded;
    IncompleteFreeSpaceExpansionRoom room;

    Boards.Line line(String stage, int q) {
      return new Boards.Line()
          .str("stage", stage)
          .num("q", q)
          .num("layer", layer)
          .num("x", x)
          .num("y", y)
          .num("h", h)
          .num("net", net)
          .bool("bounded", bounded);
    }
  }

  static void shape(StringBuilder text, TileShape shape) {
    if (shape == null) {
      text.append('-');
    } else if (shape instanceof IntOctagon o) {
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
    } else if (shape instanceof Simplex s) {
      text.append('S');
      for (int i = 0; i < s.borderLineCount(); i++) {
        Line line = s.borderLine(i);
        point(text, line.a);
        point(text, line.b);
      }
    } else {
      text.append("? ").append(shape.getClass().getSimpleName());
    }
  }

  static void point(StringBuilder text, Point point) {
    if (point instanceof IntPoint p) {
      text.append(' ').append(p.x).append(' ').append(p.y);
    } else {
      text.append(" ?").append(point);
    }
  }

  static final String TREE_NODE = "app.freerouting.datastructures.ShapeTree$TreeNode";

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
    } else if (leaf.object instanceof app.freerouting.board.model.items.Item item) {
      text.append("I ").append(item.getId());
    } else {
      text.append("? ").append(leaf.object);
    }
    text.append(' ').append(leaf.shapeIndexInObject);
  }

  /** The tree's fingerprint, as {@code TreeProbe.hash} (B02.1's walk text). */
  static void hash(Boards.Out out, String stage, String step, ShapeSearchTree tree, int rooms)
      throws Exception {
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
            .str("step", step)
            .num("rooms", rooms)
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
