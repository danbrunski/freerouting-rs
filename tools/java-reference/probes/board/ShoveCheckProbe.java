import app.freerouting.Freerouting;
import app.freerouting.board.actions.ForcedPadRouter;
import app.freerouting.board.actions.ForcedViaInserter;
import app.freerouting.core.library.Padstack;
import app.freerouting.datastructures.WorkLimit;
import app.freerouting.datastructures.WorkMeter;
import app.freerouting.geometry.planar.ConvexShape;
import app.freerouting.rules.ViaInfo;
import app.freerouting.autoroute.expansion.CompleteFreeSpaceExpansionRoom;
import app.freerouting.board.facade.RoutingBoard;
import app.freerouting.board.model.items.Item;
import app.freerouting.board.model.items.Pin;
import app.freerouting.board.model.items.Trace;
import app.freerouting.board.model.structure.AngleRestriction;
import app.freerouting.board.model.structure.ShapeEntrySide;
import app.freerouting.board.searchtree.ShapeSearchTree;
import app.freerouting.board.optimize.TraceShover;
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
 * Board probe {@code shove-check} (O03.4, O03.6-O03.8): the shove checks the maze search asks, on
 * shove samples ({@link Sample}) and a grid of via locations.
 *
 * <p>Boards: Issue413-test, Issue676-ch32v-tx118s and Issue957-smoke-ldo-led as loaded;
 * Issue026-J2_reference and Issue508-DAC2020_bm08 with their fixtures.tsv (routed) session. The
 * probe exits 3 on every other row. Stages, in this order on one board: {@code trace-check},
 * {@code via-grid} ({@link #viaGrid}), {@code via-pins} ({@link #viaPins}) and {@code limit}
 * ({@link #limit}) with the board's own 45 degree restriction, then {@code trace-check-90}, {@code
 * via-grid-90} and {@code via-pins-90} after {@code
 * rules.setTraceAngleRestriction(NINETY_DEGREE)} (forced 90 degree; the sampler then draws 4
 * directions).
 *
 * <p>Stage {@code trace-check}: for each of {@value #SAMPLES} samples {@code i} the {@code sample}
 * line, then with {@code nets} the trace's nets for even {@code i} and the next net for odd
 * {@code i} ({@link #foreign}), and {@code fromSide} the segment constructor shoving left for even
 * {@code i}: one line per query, each with the result, the board's shove-failing obstacle id (-1
 * for none) and layer, and {@code maxGeneratedId}: {@code deep}, {@code new
 * TraceShover(board).check(shape, fromSide, null, layer, nets, class, 20, 8, 5, null)}; {@code
 * flat}, the same with depths (0, 0, 0); {@code okLeft} and {@code okRight}, the static {@code
 * TraceShover.check(board, lineSegment, left, layer, nets, halfWidth, class, 20, 8)} (raw bits);
 * {@code okLeft0} and {@code okRight0}, the same with via depth 0 (the vias in the way then cut
 * the OK length short);
 * {@code forcedDeep} and {@code forcedFlat}, {@code board.checkForcedTracePolyline(new
 * Polyline(from, to), halfWidth, layer, nets, class, 20, 8, 5)} and with depths (0, 0, 0).
 */
public class ShoveCheckProbe {
  static final int SAMPLES = 300;
  static final Map<String, Boolean> BOARDS =
      Map.of(
          "Issue413-test", false,
          "Issue676-ch32v-tx118s", false,
          "Issue957-smoke-ldo-led", false,
          "Issue026-J2_reference", true,
          "Issue508-DAC2020_bm08", true);

  public static void main(String[] args) throws Exception {
    String board = Boards.board(args[0]);
    Boolean withSession = BOARDS.get(board);
    if (withSession == null) {
      System.exit(3);
    }
    Boards.Out out = new Boards.Out("shove-check", args[0]);
    try {
      RoutingBoard routingBoard = Boards.load(args[0]);
      if (withSession) {
        Boards.importSession(routingBoard, args[1]);
      }
      traceCheck(out, routingBoard, "trace-check");
      viaGrid(out, routingBoard, "via-grid");
      viaPins(out, routingBoard, "via-pins");
      limit(out, routingBoard);
      routingBoard.rules.setTraceAngleRestriction(AngleRestriction.NINETY_DEGREE);
      traceCheck(out, routingBoard, "trace-check-90");
      viaGrid(out, routingBoard, "via-grid-90");
      viaPins(out, routingBoard, "via-pins-90");
    } catch (Throwable t) {
      out.exception(t);
    }
    out.write(args[2]);
  }

  /** The net after the trace's first net (wrapping), so that the sampled trace is foreign. */
  static int[] foreign(RoutingBoard board, int[] nets) {
    int max = board.rules.nets.maxNetNumber();
    int first = nets.length > 0 ? nets[0] : 0;
    return new int[] {max > 0 ? first % max + 1 : 1};
  }

  static Boards.Line state(Boards.Line line, RoutingBoard board) {
    Item failing = board.getShoveFailingObstacle();
    return line.num("failing", failing == null ? -1 : failing.getId())
        .num("failingLayer", board.getShoveFailingLayer())
        .num("maxGeneratedId", board.communication.idGenerator.maxGeneratedId());
  }

  static void traceCheck(Boards.Out out, RoutingBoard board, String stage) {
    out.line(new Boards.Line().str("stage", stage).end());
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
      int[] nets = i % 2 == 0 ? s.nets : foreign(board, s.nets);
      ShapeEntrySide fromSide = new ShapeEntrySide(s.lineSegment, s.shape, i % 2 == 0);
      boolean deep =
          new TraceShover(board)
              .check(s.shape, fromSide, null, s.layer, nets, s.clearanceClass, 20, 8, 5, null);
      out.line(state(new Boards.Line().num("i", i).str("q", "deep").bool("ok", deep), board).end());
      boolean flat =
          new TraceShover(board)
              .check(s.shape, fromSide, null, s.layer, nets, s.clearanceClass, 0, 0, 0, null);
      out.line(state(new Boards.Line().num("i", i).str("q", "flat").bool("ok", flat), board).end());
      for (int viaDepth : new int[] {8, 0}) {
        for (boolean left : new boolean[] {true, false}) {
          double length =
              TraceShover.check(
                  board, s.lineSegment, left, s.layer, nets, s.trace.getHalfWidth(),
                  s.clearanceClass, 20, viaDepth);
          String q = (left ? "okLeft" : "okRight") + (viaDepth == 0 ? "0" : "");
          out.line(
              state(new Boards.Line().num("i", i).str("q", q).dbl("length", length), board).end());
        }
      }
      Polyline polyline = new Polyline(s.from, s.to);
      for (int[] depths : new int[][] {{20, 8, 5}, {0, 0, 0}}) {
        boolean ok =
            board.checkForcedTracePolyline(
                polyline, s.trace.getHalfWidth(), s.layer, nets, s.clearanceClass, depths[0],
                depths[1], depths[2]);
        String q = depths[0] > 0 ? "forcedDeep" : "forcedFlat";
        out.line(state(new Boards.Line().num("i", i).str("q", q).bool("ok", ok), board).end());
      }
    }
  }

  /** The cells of the via grid per side. */
  static final int GRID = 20;

  /**
   * Stage {@code via-grid}: the first via of the board's first via rule at the centre of each cell
   * {@code k = row * GRID + column} of a {@value #GRID} x {@value #GRID} grid over the board's
   * bounding box ({@code x = ll.x + (2 * column + 1) * width / (2 * GRID)} in long arithmetic, the
   * same for y), with {@code nets = {1 + k % maxNetNumber}}: the cell line, then {@link #viaQueries}.
   * Each query with the shove-failing obstacle, layer and {@code maxGeneratedId}.
   */
  static void viaGrid(Boards.Out out, RoutingBoard board, String stage) {
    out.line(new Boards.Line().str("stage", stage).end());
    ViaInfo via = firstVia(board);
    if (via == null) {
      return;
    }
    IntBox box = board.getBoundingBox();
    int maxNet = board.rules.nets.maxNetNumber();
    for (int k = 0; k < GRID * GRID; k++) {
      int row = k / GRID;
      int column = k % GRID;
      int x = (int) (box.ll.x + (2L * column + 1) * ((long) box.ur.x - box.ll.x) / (2 * GRID));
      int y = (int) (box.ll.y + (2L * row + 1) * ((long) box.ur.y - box.ll.y) / (2 * GRID));
      IntPoint location = new IntPoint(x, y);
      int[] nets = new int[] {maxNet > 0 ? 1 + k % maxNet : 1};
      out.line(new Boards.Line().num("k", k).str("at", point(location)).end());
      viaQueries(out, board, via, k, location, nets, false);
    }
  }

  /** The pins of stage {@code via-pins}. */
  static final int PINS = 100;

  /**
   * Stage {@code via-pins}: the queries of {@link #viaGrid} at the centres of the first {@value
   * #PINS} pins with a net, by ascending id ({@code k} counts them), with the pin's nets: vias on
   * and next to own-net copper; each {@code checkLayer} also with {@code attachSmdAllowed} true
   * ({@code layerAttach}).
   */
  static void viaPins(Boards.Out out, RoutingBoard board, String stage) {
    out.line(new Boards.Line().str("stage", stage).end());
    ViaInfo via = firstVia(board);
    if (via == null) {
      return;
    }
    List<Pin> pins = new ArrayList<>();
    for (Item item : board.getItems()) {
      if (item instanceof Pin pin && pin.netCount() > 0) {
        pins.add(pin);
      }
    }
    pins.sort((p, q) -> Integer.compare(p.getId(), q.getId()));
    for (int k = 0; k < pins.size() && k < PINS; k++) {
      Pin pin = pins.get(k);
      Point location = pin.getCenter();
      out.line(
          new Boards.Line().num("k", k).num("pin", pin.getId()).str("at", point(location)).end());
      viaQueries(out, board, via, k, location, pin.netNumbers.clone(), true);
    }
  }

  /** The first via of the board's first via rule; null if there is none. */
  static ViaInfo firstVia(RoutingBoard board) {
    if (board.rules.viaRules.isEmpty() || board.rules.viaRules.get(0).viaCount() == 0) {
      return null;
    }
    return board.rules.viaRules.get(0).getVia(0);
  }

  /**
   * The via queries at one location {@code k}: {@code via}, {@code ForcedViaInserter.check(via,
   * location, nets, 20, 8, board, pens, class)} with {@code pens} the minimum trace half width on
   * every layer for even {@code k} and {@code null} for odd {@code k}; then per padstack layer
   * {@code layer}, {@code ForcedViaInserter.checkLayer} as the maze search calls it (radius {@code
   * max(0.5 * shape.maxWidth(), minHalfWidth)}, the via's class and attach flag, depths 20 and 0,
   * trace half width {@code minHalfWidth}, trace class the via's) in the room {@link #room}.
   */
  static void viaQueries(
      Boards.Out out,
      RoutingBoard board,
      ViaInfo via,
      int k,
      Point location,
      int[] nets,
      boolean attachToo) {
    Padstack padstack = via.getPadstack();
    int halfWidth = board.getMinTraceHalfWidth();
    int[] pens = new int[board.getLayerCount()];
    java.util.Arrays.fill(pens, halfWidth);
    boolean ok =
        ForcedViaInserter.check(
            via, location, nets, 20, 8, board, k % 2 == 0 ? pens : null,
            via.getClearanceClassIndex());
    out.line(state(new Boards.Line().num("k", k).str("q", "via").bool("ok", ok), board).end());
    if (!(location instanceof IntPoint intLocation)) {
      return;
    }
    for (int layer = padstack.fromLayer(); layer <= padstack.toLayer(); layer++) {
      ConvexShape shape = padstack.getShape(layer);
      double viaRadius = shape == null ? 0 : 0.5 * shape.maxWidth();
      double radius = Math.max(viaRadius, halfWidth);
      int clearance =
          board.clearanceValue(via.getClearanceClassIndex(), via.getClearanceClassIndex(), layer);
      TileShape room =
          room(k, intLocation, 4 * (int) Math.ceil(radius) + 4 * halfWidth + clearance);
      for (boolean attach : attachToo ? new boolean[] {via.attachSmdAllowed(), true}
          : new boolean[] {via.attachSmdAllowed()}) {
        ForcedPadRouter.CheckDrillResult result =
            ForcedViaInserter.checkLayer(
                radius, via.getClearanceClassIndex(), attach, room, location, layer, nets, 20, 0,
                board, halfWidth, via.getClearanceClassIndex());
        out.line(
            state(
                    new Boards.Line()
                        .num("k", k)
                        .str("q", attach != via.attachSmdAllowed() ? "layerAttach" : "layer")
                        .num("layer", layer)
                        .str("room", shape(room))
                        .str("result", result.name()),
                    board)
                .end());
      }
    }
  }

  /**
   * The room of grid cell {@code k} around {@code p} with reach {@code s}, by {@code k % 7}: a box
   * reaching {@code s} on every side; reaching only up, only left, only right (1 on the other
   * sides); an octagon along the rising diagonal ({@code x - y} within 1 of p's), along the falling
   * diagonal ({@code x + y} within 1); a box of reach 1. So {@code calculateFromSide} takes each
   * branch, and fails on the last.
   */
  static TileShape room(int k, IntPoint p, int s) {
    int x = p.x;
    int y = p.y;
    return switch (k % 7) {
      case 0 -> new IntBox(x - s, y - s, x + s, y + s);
      case 1 -> new IntBox(x - 1, y - 1, x + 1, y + s);
      case 2 -> new IntBox(x - s, y - 1, x + 1, y + 1);
      case 3 -> new IntBox(x - 1, y - 1, x + s, y + 1);
      case 4 ->
          new IntOctagon(x - s, y - s, x + s, y + s, x - y - 1, x - y + 1, x + y - 2 * s,
                  x + y + 2 * s)
              .normalize();
      case 5 ->
          new IntOctagon(x - s, y - s, x + s, y + s, x - y - 2 * s, x - y + 2 * s, x + y - 1,
                  x + y + 1)
              .normalize();
      default -> new IntBox(x - 1, y - 1, x + 1, y + 1);
    };
  }

  /** The samples of stage {@code limit}. */
  static final int LIMIT_SAMPLES = 50;

  /**
   * Stage {@code limit}: on the first {@value #LIMIT_SAMPLES} samples with a shape (nets and from
   * side as in {@code trace-check}), the deep check without a limit ({@code free}: result and the
   * work units it took, {@code u}), then with {@code new WorkLimit(1, "probe", null)} after padding
   * work ({@code overlappingItemsWithClearance} of the sample's shape until the units since the
   * limit started reach {@code 64000 - u * (1 + i % 3) / 4}), so that the limit fires a quarter,
   * half or three quarters through the check ({@code limited}: result, the units since the limit
   * started, whether it is exceeded). Each with the shove-failing obstacle, layer and {@code
   * maxGeneratedId}.
   */
  static void limit(Boards.Out out, RoutingBoard board) {
    out.line(new Boards.Line().str("stage", "limit").end());
    List<Sample> samples = samples(board, SAMPLES);
    WorkMeter meter = WorkMeter.current();
    int done = 0;
    for (int i = 0; i < samples.size() && done < LIMIT_SAMPLES; i++) {
      Sample s = samples.get(i);
      if (s.shape == null) {
        continue;
      }
      done++;
      int[] nets = i % 2 == 0 ? s.nets : foreign(board, s.nets);
      ShapeEntrySide fromSide = new ShapeEntrySide(s.lineSegment, s.shape, i % 2 == 0);
      long before = meter.units();
      boolean free =
          new TraceShover(board)
              .check(s.shape, fromSide, null, s.layer, nets, s.clearanceClass, 20, 8, 5, null);
      long units = meter.units() - before;
      out.line(
          state(
                  new Boards.Line().num("i", i).str("q", "free").bool("ok", free).num("u", units),
                  board)
              .end());
      long start = meter.units();
      WorkLimit workLimit = new WorkLimit(1, "probe", null);
      long pad = 64_000 - units * (1 + i % 3) / 4;
      while (meter.units() - start < pad) {
        long last = meter.units();
        board.overlappingItemsWithClearance(s.shape, s.layer, new int[0], s.clearanceClass);
        if (meter.units() == last) {
          break;
        }
      }
      boolean limited =
          new TraceShover(board)
              .check(
                  s.shape, fromSide, null, s.layer, nets, s.clearanceClass, 20, 8, 5, workLimit);
      long used = meter.units() - start;
      boolean exceeded = workLimit.limitExceeded();
      out.line(
          state(
                  new Boards.Line()
                      .num("i", i)
                      .str("q", "limited")
                      .bool("ok", limited)
                      .num("u", used)
                      .bool("exceeded", exceeded),
                  board)
              .end());
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
