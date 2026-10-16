import app.freerouting.Freerouting;
import app.freerouting.board.facade.RoutingBoard;
import app.freerouting.board.model.items.Item;
import app.freerouting.board.model.items.Trace;
import app.freerouting.board.model.structure.AngleRestriction;
import app.freerouting.board.model.structure.ShapeAndEntrySide;
import app.freerouting.board.model.structure.ShapeEntrySide;
import app.freerouting.board.trace.PolylineTrace;
import app.freerouting.core.RoutingJob;
import app.freerouting.geometry.planar.FloatPoint;
import app.freerouting.geometry.planar.IntBox;
import app.freerouting.geometry.planar.IntOctagon;
import app.freerouting.geometry.planar.IntPoint;
import app.freerouting.geometry.planar.Line;
import app.freerouting.geometry.planar.LineSegment;
import app.freerouting.geometry.planar.Point;
import app.freerouting.geometry.planar.Polyline;
import app.freerouting.geometry.planar.TileShape;
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
import java.util.Random;
import java.util.Set;
import java.util.UUID;

/**
 * Board probe {@code entry-side} (O03.1): the four {@code ShapeEntrySide} constructors and {@code
 * ShapeAndEntrySide} on {@value #SAMPLES} shove samples ({@link Sample}).
 *
 * <p>Boards: Issue026-J2_reference with its fixtures.tsv session and Issue413-test as loaded; the
 * probe exits 3 on every other row.
 *
 * <p>Per sample {@code i}, one {@code sample} line (trace, segment, end points, shape), then one
 * line per constructor: {@code polyline} ({@code (t.polyline(), segment, shape)}), {@code point}
 * ({@code (t.polyline().corner(segment), shape)}), {@code left} and {@code right} ({@code
 * (lineSegment, shape, shoveToTheLeft)}), {@code values} ({@code (segment, cornerApprox(segment))}),
 * and {@code shapeAndSide} for {@code (t, segment - 1, orthogonal, inShoveCheck)} in the order
 * (false, false), (false, true), (true, false), (true, true). A side is {@code no} and {@code at}
 * (raw bits, or {@code null}); a {@code null} side is {@code "side":"null"}; a Java exception is
 * {@code "error":true}.
 */
public class EntrySideProbe {
  static final int SAMPLES = 300;
  static final Set<String> BOARDS = Set.of("Issue026-J2_reference", "Issue413-test");

  public static void main(String[] args) throws Exception {
    String board = Boards.board(args[0]);
    if (!BOARDS.contains(board)) {
      System.exit(3);
    }
    Boards.Out out = new Boards.Out("entry-side", args[0]);
    try {
      RoutingBoard routingBoard = Boards.load(args[0]);
      if (board.equals("Issue026-J2_reference")) {
        Boards.importSession(routingBoard, args[1]);
      }
      run(out, routingBoard);
    } catch (Throwable t) {
      out.exception(t);
    }
    out.write(args[2]);
  }

  static Boards.Line side(Boards.Line line, ShapeEntrySide side) {
    if (side == null) {
      return line.str("side", "null");
    }
    return line.num("no", side.no).str("at", floatPoint(side.borderIntersection));
  }

  interface Ctor {
    ShapeEntrySide make();
  }

  static void ctor(Boards.Out out, int i, String name, Ctor ctor) {
    Boards.Line line = new Boards.Line().num("i", i).str("ctor", name);
    try {
      line = side(line, ctor.make());
    } catch (RuntimeException e) {
      line = line.bool("error", true);
    }
    out.line(line.end());
  }

  static void run(Boards.Out out, RoutingBoard board) {
    List<Sample> samples = samples(board, SAMPLES);
    for (int i = 0; i < samples.size(); i++) {
      Sample s = samples.get(i);
      out.line(
          new Boards.Line()
              .num("i", i)
              .num("trace", s.trace.getId())
              .num("segment", s.segment)
              .str("from", point(s.from))
              .str("to", point(s.to))
              .str("shape", shape(s.shape))
              .end());
      if (s.shape == null) {
        continue;
      }
      Polyline lines = s.trace.polyline();
      ctor(out, i, "polyline", () -> new ShapeEntrySide(lines, s.segment, s.shape));
      ctor(out, i, "point", () -> new ShapeEntrySide(lines.corner(s.segment), s.shape));
      ctor(out, i, "left", () -> new ShapeEntrySide(s.lineSegment, s.shape, true));
      ctor(out, i, "right", () -> new ShapeEntrySide(s.lineSegment, s.shape, false));
      ctor(out, i, "values", () -> new ShapeEntrySide(s.segment, lines.cornerApprox(s.segment)));
      for (int k = 0; k < 4; k++) {
        boolean orthogonal = k >= 2;
        boolean check = k % 2 == 1;
        Boards.Line line =
            new Boards.Line()
                .num("i", i)
                .str("ctor", "shapeAndSide")
                .bool("orthogonal", orthogonal)
                .bool("check", check);
        try {
          ShapeAndEntrySide result =
              new ShapeAndEntrySide(s.trace, s.segment - 1, orthogonal, check);
          line = side(line.str("shape", shape(result.shape)), result.fromSide);
        } catch (RuntimeException e) {
          line = line.bool("error", true);
        }
        out.line(line.end());
      }
    }
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
