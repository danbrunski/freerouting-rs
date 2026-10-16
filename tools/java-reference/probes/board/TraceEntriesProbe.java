import app.freerouting.Freerouting;
import app.freerouting.autoroute.expansion.CompleteFreeSpaceExpansionRoom;
import app.freerouting.board.facade.RoutingBoard;
import app.freerouting.board.model.items.Item;
import app.freerouting.board.model.items.Trace;
import app.freerouting.board.model.structure.AngleRestriction;
import app.freerouting.board.model.structure.ShapeEntrySide;
import app.freerouting.board.searchtree.ShapeSearchTree;
import app.freerouting.board.searchtree.ShapeTraceEntries;
import app.freerouting.board.trace.PolylineTrace;
import app.freerouting.core.RoutingJob;
import app.freerouting.datastructures.ShapeTree;
import app.freerouting.geometry.planar.FloatPoint;
import app.freerouting.geometry.planar.IntBox;
import app.freerouting.geometry.planar.IntOctagon;
import app.freerouting.geometry.planar.IntPoint;
import app.freerouting.geometry.planar.Line;
import app.freerouting.geometry.planar.LineSegment;
import app.freerouting.geometry.planar.Point;
import app.freerouting.geometry.planar.Polyline;
import app.freerouting.geometry.planar.RegularTileShape;
import app.freerouting.geometry.planar.TileShape;
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
import java.util.Collection;
import java.util.Deque;
import java.util.List;
import java.util.Map;
import java.util.Random;
import java.util.UUID;

/**
 * Board probe {@code trace-entries} (O03.2, O03.3): {@code ShapeTraceEntries} on shove samples
 * ({@link Sample}).
 *
 * <p>Boards: Issue026-J2_reference with its fixtures.tsv session; Issue413-test,
 * Issue676-ch32v-tx118s and Issue957-smoke-ldo-led as loaded (their DSN files hold wiring). The
 * probe exits 3 on every other row.
 *
 * <p>Stage {@code store}, on the board: for each of {@value #STORE_SAMPLES} samples {@code i}, the
 * {@code sample} line; then the obstacles {@code
 * defaultTree.overlappingItemsWithClearance(shape, layer, new int[0], class)} (a {@code TreeSet},
 * highest id first) go through {@code new ShapeTraceEntries(shape, layer, nets, class, fromSide,
 * board).storeItems(obstacles, i % 5 == 4, i % 7 != 6)}, with {@code fromSide} the segment
 * constructor shoving left for {@code i % 3 == 0}, right for 1 and {@code null} for 2, and {@code
 * nets} the trace's nets for even {@code i} and {@link #foreign} for odd {@code i}. One line
 * with the obstacles, the result, the found obstacle (-1 for none), {@code traceTailsInShape},
 * {@code stackDepth}, {@code substituteTraceCount} and the shove vias; when the store succeeded,
 * one line per {@code nextSubstituteTracePiece} (id, half width, nets, lines) until it returns
 * {@code null}; then {@code maxGeneratedId}.
 *
 * <p>Stage {@code cutout}, on a fresh load: for each of the first {@value #CUTOUT_SAMPLES}
 * samples {@code k} (drawn before any cut), {@code new ShapeTraceEntries(shape, layer,
 * foreign(nets), class, null, board).cutoutTraces(obstacles)}; one line with the obstacles, those no longer on
 * the board, the traces with an id above the previous {@code maxGeneratedId} ({@code
 * "id:lines"}, lowest id first) and {@code maxGeneratedId}; after every 10th sample the default
 * tree's walk hash (TreeProbe's {@code shapeHash}), node and leaf counts.
 */
public class TraceEntriesProbe {
  static final int STORE_SAMPLES = 300;
  static final int CUTOUT_SAMPLES = 100;
  static final Map<String, Boolean> BOARDS =
      Map.of(
          "Issue026-J2_reference", true,
          "Issue413-test", false,
          "Issue676-ch32v-tx118s", false,
          "Issue957-smoke-ldo-led", false);

  public static void main(String[] args) throws Exception {
    String board = Boards.board(args[0]);
    Boolean withSession = BOARDS.get(board);
    if (withSession == null) {
      System.exit(3);
    }
    Boards.Out out = new Boards.Out("trace-entries", args[0]);
    try {
      store(out, load(args, withSession));
      cutout(out, load(args, withSession));
    } catch (Throwable t) {
      out.exception(t);
    }
    out.write(args[2]);
  }

  static RoutingBoard load(String[] args, boolean withSession) throws Exception {
    RoutingBoard board = Boards.load(args[0]);
    if (withSession) {
      Boards.importSession(board, args[1]);
    }
    return board;
  }

  /** The net after the trace's first net (wrapping), so that the sampled trace is foreign. */
  static int[] foreign(RoutingBoard board, int[] nets) {
    int max = board.rules.nets.maxNetNumber();
    int first = nets.length > 0 ? nets[0] : 0;
    return new int[] {max > 0 ? first % max + 1 : 1};
  }

  static Boards.Line sampleLine(int i, Sample s) {
    return new Boards.Line()
        .num("i", i)
        .num("trace", s.trace.getId())
        .num("segment", s.segment)
        .str("from", point(s.from))
        .str("to", point(s.to))
        .str("shape", shape(s.shape));
  }

  static void store(Boards.Out out, RoutingBoard board) {
    out.line(
        new Boards.Line()
            .str("stage", "store")
            .num("maxGeneratedId", board.communication.idGenerator.maxGeneratedId())
            .end());
    ShapeSearchTree tree = board.searchTreeManager.getDefaultTree();
    List<Sample> samples = samples(board, STORE_SAMPLES);
    for (int i = 0; i < samples.size(); i++) {
      Sample s = samples.get(i);
      out.line(sampleLine(i, s).end());
      if (s.shape == null) {
        continue;
      }
      ShapeEntrySide fromSide =
          i % 3 == 0
              ? new ShapeEntrySide(s.lineSegment, s.shape, true)
              : i % 3 == 1 ? new ShapeEntrySide(s.lineSegment, s.shape, false) : null;
      int layer = s.trace.getLayer();
      int cl = s.trace.clearanceClassIndex();
      Collection<Item> obstacles =
          tree.overlappingItemsWithClearance(s.shape, layer, new int[0], cl);
      ShapeTraceEntries entries =
          new ShapeTraceEntries(
              s.shape, layer, i % 2 == 0 ? s.nets : foreign(board, s.nets), cl, fromSide, board);
      boolean ok = entries.storeItems(obstacles, i % 5 == 4, i % 7 != 6);
      Item found = entries.getFoundObstacle();
      out.line(
          new Boards.Line()
              .num("i", i)
              .raw("obstacles", ids(obstacles))
              .bool("ok", ok)
              .num("found", found == null ? -1 : found.getId())
              .bool("tails", entries.traceTailsInShape())
              .num("depth", entries.stackDepth())
              .num("pieces", entries.substituteTraceCount())
              .raw("vias", ids(entries.shoveViaList))
              .end());
      if (ok) {
        int k = 0;
        for (PolylineTrace piece = entries.nextSubstituteTracePiece();
            piece != null;
            piece = entries.nextSubstituteTracePiece()) {
          out.line(
              new Boards.Line()
                  .num("i", i)
                  .num("piece", k)
                  .num("id", piece.getId())
                  .num("hw", piece.getHalfWidth())
                  .raw("nets", ints(piece.netNumbers))
                  .str("lines", lines(piece.polyline()))
                  .end());
          k++;
        }
      }
      out.line(
          new Boards.Line()
              .num("i", i)
              .num("maxGeneratedId", board.communication.idGenerator.maxGeneratedId())
              .end());
    }
  }

  static void cutout(Boards.Out out, RoutingBoard board) throws Exception {
    out.line(
        new Boards.Line()
            .str("stage", "cutout")
            .num("maxGeneratedId", board.communication.idGenerator.maxGeneratedId())
            .end());
    ShapeSearchTree tree = board.searchTreeManager.getDefaultTree();
    List<Sample> samples = samples(board, CUTOUT_SAMPLES);
    for (int k = 0; k < samples.size(); k++) {
      Sample s = samples.get(k);
      if (s.shape == null) {
        continue;
      }
      int before = board.communication.idGenerator.maxGeneratedId();
      int layer = s.layer;
      Collection<Item> obstacles =
          tree.overlappingItemsWithClearance(s.shape, layer, new int[0], s.clearanceClass);
      ShapeTraceEntries entries =
          new ShapeTraceEntries(
              s.shape, layer, foreign(board, s.nets), s.clearanceClass, null, board);
      entries.cutoutTraces(obstacles);
      List<Item> gone = new ArrayList<>();
      for (Item item : obstacles) {
        if (!item.isOnTheBoard()) {
          gone.add(item);
        }
      }
      List<PolylineTrace> created = new ArrayList<>();
      for (Trace t : board.getTraces()) {
        if (t.getId() > before && t instanceof PolylineTrace pt) {
          created.add(pt);
        }
      }
      created.sort((p, q) -> Integer.compare(p.getId(), q.getId()));
      StringBuilder text = new StringBuilder("[");
      for (PolylineTrace t : created) {
        if (text.length() > 1) {
          text.append(',');
        }
        text.append('"').append(t.getId()).append(':').append(lines(t.polyline())).append('"');
      }
      text.append(']');
      out.line(
          new Boards.Line()
              .num("k", k)
              .raw("obstacles", ids(obstacles))
              .raw("gone", ids(gone))
              .raw("new", text.toString())
              .num("maxGeneratedId", board.communication.idGenerator.maxGeneratedId())
              .end());
      if (k % 10 == 9) {
        walkHash(out, k, tree);
      }
    }
  }

  static final String TREE_NODE = "app.freerouting.datastructures.ShapeTree$TreeNode";

  /** TreeProbe's walk hash of {@code tree}, with its node and leaf counts. */
  static void walkHash(Boards.Out out, int k, ShapeSearchTree tree) throws Exception {
    long shapeHash = Boards.FNV_OFFSET;
    int nodes = 0;
    int leaves = 0;
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
    out.line(
        new Boards.Line()
            .num("k", k)
            .str("walk", Boards.hex(shapeHash))
            .num("nodes", nodes)
            .num("leaves", leaves)
            .end());
  }

  static Object get(String className, String fieldName, Object target) throws Exception {
    Field field = Class.forName(className).getDeclaredField(fieldName);
    field.setAccessible(true);
    return field.get(target);
  }

  static void bounds(StringBuilder text, RegularTileShape shape) {
    if (shape instanceof IntOctagon o) {
      text.append("O ").append(o.leftX).append(' ').append(o.bottomY).append(' ')
          .append(o.rightX).append(' ').append(o.topY).append(' ')
          .append(o.upperLeftDiagonalX).append(' ').append(o.lowerRightDiagonalX).append(' ')
          .append(o.lowerLeftDiagonalX).append(' ').append(o.upperRightDiagonalX);
    } else if (shape instanceof IntBox b) {
      text.append("B ").append(b.ll.x).append(' ').append(b.ll.y).append(' ')
          .append(b.ur.x).append(' ').append(b.ur.y);
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

  // BEGIN ShoveSampler
  /**
   * The shove sampler (O03.2), mirrored by {@code crates/freerouting/tests/support/shove_sampler.rs}.
   * The traces are the board's {@code PolylineTrace}s, highest id first. {@code new
   * Random(20261015)}; per sample, in this order: the trace {@code t = traces[nextInt(count)]}; the
   * segment {@code s = 1 + nextInt(lineCount - 2)}; with {@code a, b} the approximate corners {@code
   * s - 1} and {@code s}, {@code f = nextInt(1001)}, {@code x = (int) Math.round(a.x + (b.x - a.x) *
   * f / 1000.0)} and the same for {@code y}; the direction {@code nextInt(8)} of (1,0), (1,1), (0,1),
   * (-1,1), (-1,0), (-1,-1), (0,-1), (1,-1), or {@code nextInt(4)} of (1,0), (0,1), (-1,0), (0,-1)
   * on a board with 90 degree traces; the length {@code (1 + nextInt(20)) * minTraceHalfWidth}. The
   * segment runs from (x, y) by length times the direction; the shape is {@code
   * offsetShapes(t.getHalfWidth())[0]} of the segment's polyline ({@code null} if there is not
   * exactly one). Layer, nets and clearance class are the trace's.
   */
  static final class Sample {
    final PolylineTrace trace;
    final int segment;
    final IntPoint from;
    final IntPoint to;
    final LineSegment lineSegment;
    final TileShape shape;
    final int layer;
    final int[] nets;
    final int clearanceClass;

    Sample(PolylineTrace trace, int segment, IntPoint from, IntPoint to, LineSegment lineSegment,
        TileShape shape) {
      this.trace = trace;
      this.segment = segment;
      this.from = from;
      this.to = to;
      this.lineSegment = lineSegment;
      this.shape = shape;
      this.layer = trace.getLayer();
      this.nets = trace.netNumbers.clone();
      this.clearanceClass = trace.clearanceClassIndex();
    }
  }

  static final long SAMPLER_SEED = 20261015L;
  static final int[][] DIRS45 = {{1, 0}, {1, 1}, {0, 1}, {-1, 1}, {-1, 0}, {-1, -1}, {0, -1}, {1, -1}};
  static final int[][] DIRS90 = {{1, 0}, {0, 1}, {-1, 0}, {0, -1}};

  static List<Sample> samples(RoutingBoard board, int count) {
    List<PolylineTrace> traces = new ArrayList<>();
    for (Trace t : board.getTraces()) {
      if (t instanceof PolylineTrace pt) {
        traces.add(pt);
      }
    }
    traces.sort((p, q) -> Integer.compare(q.getId(), p.getId()));
    List<Sample> result = new ArrayList<>();
    if (traces.isEmpty()) {
      return result;
    }
    int[][] dirs =
        board.rules.getTraceAngleRestriction() == AngleRestriction.NINETY_DEGREE ? DIRS90 : DIRS45;
    int unit = board.getMinTraceHalfWidth();
    Random random = new Random(SAMPLER_SEED);
    for (int i = 0; i < count; i++) {
      PolylineTrace t = traces.get(random.nextInt(traces.size()));
      Polyline lines = t.polyline();
      int segment = 1 + random.nextInt(lines.lines.length - 2);
      FloatPoint a = lines.cornerApprox(segment - 1);
      FloatPoint b = lines.cornerApprox(segment);
      int f = random.nextInt(1001);
      int x = (int) Math.round(a.x + (b.x - a.x) * f / 1000.0);
      int y = (int) Math.round(a.y + (b.y - a.y) * f / 1000.0);
      int[] dir = dirs[random.nextInt(dirs.length)];
      int length = (1 + random.nextInt(20)) * unit;
      IntPoint from = new IntPoint(x, y);
      IntPoint to = new IntPoint(x + dir[0] * length, y + dir[1] * length);
      Polyline polyline = new Polyline(from, to);
      TileShape[] shapes = polyline.offsetShapes(t.getHalfWidth());
      boolean one = shapes.length == 1;
      result.add(new Sample(t, segment, from, to, one ? new LineSegment(polyline, 1) : null,
          one ? shapes[0] : null));
    }
    return result;
  }

  /** An integer point as {@code x,y}; any other point as {@code f<x bits>,<y bits>}. */
  static String point(Point p) {
    if (p instanceof IntPoint ip) {
      return ip.x + "," + ip.y;
    }
    FloatPoint f = p.toFloat();
    return "f" + Boards.bits(f.x) + "," + Boards.bits(f.y);
  }

  /** A float point as {@code <x bits>,<y bits>}; {@code null} for none. */
  static String floatPoint(FloatPoint f) {
    return f == null ? "null" : Boards.bits(f.x) + "," + Boards.bits(f.y);
  }

  /** The lines of a polyline: the two points of each, space separated. */
  static String lines(Polyline polyline) {
    StringBuilder text = new StringBuilder();
    for (Line line : polyline.lines) {
      if (text.length() > 0) {
        text.append(' ');
      }
      text.append(point(line.a)).append(' ').append(point(line.b));
    }
    return text.toString();
  }

  /**
   * A tile shape: {@code box llx lly urx ury}, {@code oct} and the eight octagon values, or {@code
   * simplex} and the two points of each border line; {@code null} for none.
   */
  static String shape(TileShape s) {
    if (s == null) {
      return "null";
    }
    if (s instanceof IntBox b) {
      return "box " + b.ll.x + " " + b.ll.y + " " + b.ur.x + " " + b.ur.y;
    }
    if (s instanceof IntOctagon o) {
      return "oct " + o.leftX + " " + o.bottomY + " " + o.rightX + " " + o.topY + " "
          + o.upperLeftDiagonalX + " " + o.lowerRightDiagonalX + " " + o.lowerLeftDiagonalX + " "
          + o.upperRightDiagonalX;
    }
    StringBuilder text = new StringBuilder("simplex");
    for (int i = 0; i < s.borderLineCount(); i++) {
      Line line = s.borderLine(i);
      text.append(' ').append(point(line.a)).append(' ').append(point(line.b));
    }
    return text.toString();
  }

  /** Item ids in iteration order, as a JSON array. */
  static String ids(Collection<? extends Item> items) {
    StringBuilder text = new StringBuilder("[");
    for (Item item : items) {
      if (text.length() > 1) {
        text.append(',');
      }
      text.append(item.getId());
    }
    return text.append(']').toString();
  }

  /** Ints as a JSON array. */
  static String ints(int[] values) {
    StringBuilder text = new StringBuilder("[");
    for (int v : values) {
      if (text.length() > 1) {
        text.append(',');
      }
      text.append(v);
    }
    return text.append(']').toString();
  }
  // END ShoveSampler

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
