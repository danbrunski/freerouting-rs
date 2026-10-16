package app.freerouting.board.optimize;

import app.freerouting.Freerouting;
import app.freerouting.board.facade.RoutingBoard;
import app.freerouting.board.model.items.Item;
import app.freerouting.board.model.items.Pin;
import app.freerouting.board.model.items.Via;
import app.freerouting.board.model.structure.AngleRestriction;
import app.freerouting.board.state.ChangedArea;
import app.freerouting.board.trace.PolylineTrace;
import app.freerouting.core.RoutingJob;
import app.freerouting.datastructures.Stoppable;
import app.freerouting.datastructures.WorkMeter;
import app.freerouting.geometry.planar.FloatPoint;
import app.freerouting.geometry.planar.IntBox;
import app.freerouting.geometry.planar.IntOctagon;
import app.freerouting.geometry.planar.IntPoint;
import app.freerouting.geometry.planar.Line;
import app.freerouting.geometry.planar.Point;
import app.freerouting.geometry.planar.Polyline;
import app.freerouting.io.specctra.SesReader;
import app.freerouting.management.BoardLoader;
import app.freerouting.settings.GlobalSettings;
import app.freerouting.autoroute.expansion.CompleteFreeSpaceExpansionRoom;
import app.freerouting.board.searchtree.ShapeSearchTree;
import app.freerouting.datastructures.ShapeTree;
import app.freerouting.geometry.planar.RegularTileShape;
import java.lang.reflect.Field;
import java.util.ArrayDeque;
import java.util.Arrays;
import java.util.Deque;
import java.io.FileInputStream;
import java.io.InputStream;
import java.lang.reflect.Method;
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
 * Board probe {@code pull-tight} (O01.7): Java's pull-tight optimizer ({@code TraceTightener45},
 * {@code TraceTightener90}, {@code PolylineTrace.pullTight}, {@code optChangedArea}, {@code
 * removeItemsAndPullTight}) on fixtures with their routed sessions. Declared in Java's {@code
 * board.optimize} package to reach the package-private steps; the private ones go through
 * reflection. The board-level modes first {@code unfix()} every trace and via, because {@code
 * SesReader} imports them {@code USER_FIXED}. One section per mode, each starting with a {@code stage} line; boards outside every
 * mode's set exit 3.
 *
 * <ul>
 *   <li>{@code steps-45}, {@code steps-90} (J2, DAC2020_bm08): 200 seeded polylines from {@code
 *       Random(20261016)} (a trace's own polyline, a bump, jittered corners, a zigzag or a
 *       line inserted through a corner; every third with a clip box), each step of the variant applied alone to the input with the
 *       trace's data, then the full fixpoint; per step the "changed" flag (a new object), the
 *       result's corners and the changed area on the trace's layer. 90 forces {@code
 *       setTraceAngleRestriction(NINETY_DEGREE)}.
 *   <li>{@code ends-45} (same boards): {@code smoothenStartCornerAtTrace} and {@code
 *       smoothenEndCornerAtTrace} of every trace, with the data {@code smoothenEndCornersAtTrace}
 *       sets and no contact pins (the imported traces are user-fixed, so a contact trace ends the
 *       smoothing); {@code ends-45-split} the same on the unfixed board after every trace with 3
 *       or more corners was split at its middle corner ({@code Trace.split(Point)}), so that trace ends
 *       meet at bends.
 *   <li>{@code board-45}, {@code board-90} ({@link #BOARD_SET}): {@code startMarkingChangedArea();
 *       markAllChangedArea(); optChangedArea(new int[0], null, 500, null, null, 0)}, then the
 *       canonical item dump; on J2 and DAC2020_bm08 also the default tree's fingerprint ({@code
 *       board-45-tree}).
 *   <li>{@code pin-exit} (J2, DAC2020_bm08, Issue143-rpi_splitter): {@code pinEdgeToTurnDist} = 4
 *       × the minimum trace half width, then {@code pullTight(true, 500, null)} on every trace
 *       ending at a pin, highest id first, then the dump.
 *   <li>{@code remove-<width>} (J2): {@code removeItemsAndPullTight} of 10 traces chosen by {@code
 *       Random(20261017)} with tidy widths {@code Integer.MAX_VALUE}, 0 and 2000, then the dump.
 *   <li>{@code limit} (DAC2020_bm08; on J2 the whole run takes fewer units than one nominal ms):
 *       {@code optChangedArea} of the whole board with a work limit of {@link #LIMIT_MS} nominal
 *       ms; the number of stop checks, the work units at the last one (the cut-off) and the dump.
 * </ul>
 *
 * <pre>
 * tools/java-reference/probe.sh &lt;pin&gt; tools/java-reference/probes/board/PullTightProbe.java \
 *     board.dsn board.ses out.jsonl
 * </pre>
 */
public class PullTightProbe {
  static final Set<String> STEP_BOARDS = Set.of("Issue026-J2_reference", "Issue508-DAC2020_bm08");
  static final Set<String> PIN_EXIT_BOARDS =
      Set.of("Issue026-J2_reference", "Issue508-DAC2020_bm08", "Issue143-rpi_splitter");
  static final String J2 = "Issue026-J2_reference";
  static final String BM08 = "Issue508-DAC2020_bm08";
  static final int ACCURACY = 500;
  static final int SAMPLES = 200;
  static final int LIMIT_MS = 1;
  static final Set<String> BOARD_SET = Set.of(
      "empty_board",
      "scoring-perfect-two-pin",
      "Issue756-minimal-ok",
      "Issue757-minimal-soe-ok",
      "Issue954-circle",
      "Issue756-minimal-hang",
      "Issue757-minimal-soe",
      "scoring-empty-net",
      "scoring-zero-length",
      "scoring-mixed-layer",
      "Issue508-SMD-routing-issue-demo",
      "Issue269-min_fr_test/min_fr_test_no_quotes",
      "Issue269-min_fr_test/min_fr_test",
      "Issue368-CorneyIslandWireless_input_design",
      "Issue143-rpi_splitter",
      "Issue270-non-ansi_bracket",
      "Issue143-rpi_splitter_mod",
      "PCBench-retroreflectors-CONGFLOCK",
      "Issue954-invalid-polygon64",
      "Issue954-polygon64",
      "Issue368-CorneyIslandWireless/corney_island_wireless",
      "Issue069-TestSensel/TestSensel-KiCad6",
      "Issue508-DAC2020/DAC2020_bm08/DAC2020_bm08.unrouted",
      "Issue508-DAC2020/DAC2020_bm08/FRv1.9.0/DAC2020_bm08.unrouted",
      "Issue508-DAC2020/DAC2020_bm08/FRv2.1.0/DAC2020_bm08.unrouted",
      "Issue508-DAC2020_bm08",
      "Issue069-TestSensel/TestSensel",
      "Issue110-testProjectFromFreeroutingBugTest",
      "Issue026-J2_reference",
      "Issue433-my-board",
      "Issue413-test",
      "Issue110-testProjectFromFreeroutingBugTest01",
      "Issue508-DAC2020/DAC2020_bm07/DAC2020_bm07.unrouted",
      "Issue508-DAC2020/DAC2020_bm07/FRv1.9.0/DAC2020_bm07.unrouted",
      "Issue508-DAC2020/DAC2020_bm07/FRv2.1.0/DAC2020_bm07.unrouted",
      "Issue508-DAC2020_bm07",
      "Issue957-smoke-ldo-led",
      "Issue508-DAC2020/DAC2020_bm05/DAC2020_bm05.unrouted",
      "Issue508-DAC2020/DAC2020_bm05/FRv1.9.0/DAC2020_bm05.unrouted",
      "Issue508-DAC2020/DAC2020_bm05/FRv2.1.0/DAC2020_bm05.unrouted",
      "Issue508-DAC2020_bm05");
  // board set: 41 boards

  public static void main(String[] args) throws Exception {
    String board = Boards.board(args[0]);
    boolean steps = STEP_BOARDS.contains(board);
    boolean full = BOARD_SET.contains(board);
    boolean pinExit = PIN_EXIT_BOARDS.contains(board);
    boolean j2 = board.equals(J2);
    boolean bm08 = board.equals(BM08);
    if (args[1].equals("-") || !(steps || full || pinExit)) {
      System.exit(3);
    }
    Boards.Out out = new Boards.Out("pull-tight", args[0]);
    try {
      if (steps) {
        steps(out, args, AngleRestriction.FORTYFIVE_DEGREE);
        steps(out, args, AngleRestriction.NINETY_DEGREE);
        ends(out, args, false);
        ends(out, args, true);
      }
      if (full) {
        boardMode(out, args, AngleRestriction.FORTYFIVE_DEGREE, steps);
        boardMode(out, args, AngleRestriction.NINETY_DEGREE, false);
      }
      if (pinExit) {
        pinExit(out, args);
      }
      if (j2) {
        for (int width : new int[] {Integer.MAX_VALUE, 0, 2000}) {
          remove(out, args, width);
        }
      }
      if (bm08) {
        limit(out, args);
      }
    } catch (Throwable t) {
      out.exception(t);
    }
    out.write(args[2]);
  }

  static RoutingBoard loaded(String[] args, AngleRestriction angle) throws Exception {
    RoutingBoard board = Boards.load(args[0]);
    Boards.importSession(board, args[1]);
    board.rules.setTraceAngleRestriction(angle);
    return board;
  }

  /**
   * {@code loaded}, then every trace and via {@code unfix()}ed: {@code SesReader} imports the
   * session's wires and vias {@code USER_FIXED}, which pull-tight and removal leave alone.
   */
  static RoutingBoard unfixed(String[] args, AngleRestriction angle) throws Exception {
    RoutingBoard board = loaded(args, angle);
    for (Item item : board.getItems()) {
      if (item instanceof PolylineTrace || item instanceof Via) {
        item.unfix();
      }
    }
    return board;
  }

  static String angleName(AngleRestriction angle) {
    return angle == AngleRestriction.NINETY_DEGREE ? "90" : "45";
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

  static String octagon(IntOctagon o) {
    return o.leftX + " " + o.bottomY + " " + o.rightX + " " + o.topY + " " + o.upperLeftDiagonalX
        + " " + o.lowerRightDiagonalX + " " + o.lowerLeftDiagonalX + " " + o.upperRightDiagonalX;
  }

  // ---- steps ----

  static List<PolylineTrace> tracesAscending(RoutingBoard board) {
    List<PolylineTrace> traces = new ArrayList<>();
    for (Item item : board.getItems()) {
      if (item instanceof PolylineTrace trace) {
        traces.add(trace);
      }
    }
    traces.sort((a, b) -> Integer.compare(a.getId(), b.getId()));
    return traces;
  }

  static boolean allInt(Polyline polyline) {
    for (int i = 0; i < polyline.cornerCount(); i++) {
      if (!(polyline.corner(i) instanceof IntPoint)) {
        return false;
      }
    }
    return true;
  }

  /** The point at distance h to the left of the segment p to q, translated from base. */
  static IntPoint offset(IntPoint base, IntPoint p, IntPoint q, int h) {
    int dx = q.x - p.x;
    int dy = q.y - p.y;
    double len = Math.sqrt((double) dx * dx + (double) dy * dy);
    int nx = (int) Math.round((double) (-dy) * h / len);
    int ny = (int) Math.round((double) dx * h / len);
    return new IntPoint(base.x + nx, base.y + ny);
  }

  /** The sample polyline's corners; null for the trace's own polyline. */
  static List<IntPoint> sampleCorners(Random random, PolylineTrace trace, int variant) {
    Polyline own = trace.polyline();
    int n = own.cornerCount();
    List<IntPoint> c = new ArrayList<>();
    for (int i = 0; i < n; i++) {
      c.add((IntPoint) own.corner(i));
    }
    int w = Math.max(trace.getHalfWidth(), 1);
    List<IntPoint> result = new ArrayList<>();
    if (variant == 1 || variant == 3) {
      int j = random.nextInt(n - 1);
      int h = (random.nextInt(6) + 1) * 2 * w * (random.nextBoolean() ? 1 : -1);
      IntPoint p = c.get(j);
      IntPoint q = c.get(j + 1);
      for (int i = 0; i <= j; i++) {
        result.add(c.get(i));
      }
      if (variant == 1) {
        result.add(offset(p, p, q, h));
        result.add(offset(q, p, q, h));
      } else {
        IntPoint mid = new IntPoint((p.x + q.x) / 2, (p.y + q.y) / 2);
        result.add(offset(mid, p, q, h));
      }
      for (int i = j + 1; i < n; i++) {
        result.add(c.get(i));
      }
    } else {
      result.add(c.get(0));
      for (int i = 1; i < n - 1; i++) {
        IntPoint p = c.get(i);
        if (random.nextBoolean()) {
          int dx = random.nextInt(2 * w + 1) - w;
          int dy = random.nextInt(2 * w + 1) - w;
          p = new IntPoint(p.x + dx, p.y + dy);
        }
        result.add(p);
      }
      result.add(c.get(n - 1));
    }
    return result;
  }

  static IntOctagon clipOf(List<IntPoint> corners) {
    int mid = corners.size() / 2;
    int llx = Integer.MAX_VALUE;
    int lly = Integer.MAX_VALUE;
    int urx = Integer.MIN_VALUE;
    int ury = Integer.MIN_VALUE;
    for (int i = 0; i <= mid; i++) {
      IntPoint p = corners.get(i);
      llx = Math.min(llx, p.x);
      lly = Math.min(lly, p.y);
      urx = Math.max(urx, p.x);
      ury = Math.max(ury, p.y);
    }
    return new IntBox(llx, lly, urx, ury).toIntOctagon();
  }

  static Polyline invoke(Method method, Object target, Polyline input) throws Exception {
    return (Polyline) method.invoke(target, input);
  }

  static void steps(Boards.Out out, String[] args, AngleRestriction angle) throws Exception {
    RoutingBoard board = loaded(args, angle);
    boolean ninety = angle == AngleRestriction.NINETY_DEGREE;
    out.line(new Boards.Line().str("stage", "steps-" + angleName(angle)).end());
    List<PolylineTrace> traces = tracesAscending(board);
    Random random = new Random(20261016L);
    Class<?> variantClass = ninety ? TraceTightener90.class : TraceTightener45.class;
    String[] names =
        ninety
            ? new String[] {"trySkipSecondCorner", "trySkipCorners"}
            : new String[] {"reduceCorners", "smoothenCorners"};
    Method first = variantClass.getDeclaredMethod(names[0], Polyline.class);
    Method second = variantClass.getDeclaredMethod(names[1], Polyline.class);
    first.setAccessible(true);
    second.setAccessible(true);
    for (int k = 0; k < SAMPLES; k++) {
      PolylineTrace trace = traces.get(random.nextInt(traces.size()));
      int variant = random.nextInt(5);
      boolean clipped = random.nextInt(3) == 2;
      if (!allInt(trace.polyline())) {
        variant = 0;
      }
      Polyline input;
      IntOctagon clip = null;
      if (variant == 0) {
        input = trace.polyline();
        if (clipped) {
          List<IntPoint> c = new ArrayList<>();
          for (int i = 0; i < input.cornerCount(); i++) {
            c.add(input.corner(i) instanceof IntPoint ip ? ip : input.corner(i).toFloat().round());
          }
          clip = clipOf(c);
        }
      } else if (variant == 4) {
        // a line through a corner: a segment of length 0 there
        Polyline own = trace.polyline();
        int j = random.nextInt(own.cornerCount());
        int dx = random.nextInt(5) - 2;
        int dy = random.nextInt(5) - 2;
        if (dx == 0 && dy == 0) {
          dx = 1;
        }
        IntPoint p = (IntPoint) own.corner(j);
        Line[] lines = new Line[own.lines.length + 1];
        System.arraycopy(own.lines, 0, lines, 0, j + 1);
        lines[j + 1] = new Line(p, new IntPoint(p.x + dx, p.y + dy));
        System.arraycopy(own.lines, j + 1, lines, j + 2, own.lines.length - j - 1);
        input = new Polyline(lines);
        if (clipped) {
          List<IntPoint> c = new ArrayList<>();
          for (int i = 0; i < own.cornerCount(); i++) {
            c.add((IntPoint) own.corner(i));
          }
          clip = clipOf(c);
        }
      } else {
        List<IntPoint> c = sampleCorners(random, trace, variant);
        input = new Polyline(c.toArray(new Point[0]));
        if (clipped) {
          clip = clipOf(c);
        }
      }
      out.line(
          new Boards.Line()
              .num("sample", k)
              .num("trace", trace.getId())
              .num("variant", variant)
              .str("clip", clip == null ? "-" : octagon(clip))
              .str("input", corners(input))
              .end());
      if (input.lines.length < 3) {
        continue;
      }
      int layer = trace.getLayer();
      TraceTightener tightener =
          TraceTightener.getInstance(board, new int[0], clip, ACCURACY, null, -1, null, -1);
      tightener.currentLayer = layer;
      tightener.currentHalfWidth =
          trace.getHalfWidth()
              + board.searchTreeManager.getDefaultTree()
                  .clearanceCompensationValue(trace.clearanceClassIndex(), layer);
      tightener.currentNetNumbers = trace.netNumbers;
      tightener.currentClearanceClassIndex = trace.clearanceClassIndex();
      tightener.contactPins = trace.touchingPinsAtEndCorners();
      for (int step = 0; step < 5; step++) {
        board.changedArea = new ChangedArea(board.getLayerCount());
        Polyline result;
        String name;
        switch (step) {
          case 0 -> {
            name = names[0];
            result = invoke(first, tightener, input);
          }
          case 1 -> {
            name = names[1];
            result = invoke(second, tightener, input);
          }
          case 2 -> {
            name = "repositionLines";
            result = tightener.repositionLines(input);
          }
          case 3 -> {
            name = "skipSegmentsOfLength0";
            result = tightener.skipSegmentsOfLength0(input);
          }
          default -> {
            name = "pullTight";
            result = tightener.pullTight(input);
          }
        }
        out.line(
            new Boards.Line()
                .str("step", name)
                .bool("changed", result != input)
                .str("corners", result == input ? "=" : corners(result))
                .str("area", octagon(board.changedArea.getArea(layer)))
                .end());
      }
      board.changedArea = null;
    }
  }

  static void ends(Boards.Out out, String[] args, boolean split) throws Exception {
    RoutingBoard board =
        split
            ? unfixed(args, AngleRestriction.FORTYFIVE_DEGREE)
            : loaded(args, AngleRestriction.FORTYFIVE_DEGREE);
    out.line(new Boards.Line().str("stage", split ? "ends-45-split" : "ends-45").end());
    if (split) {
      // split every trace with 3 or more corners at its middle corner, so that
      // trace ends meet at the trace's own bends
      for (PolylineTrace trace : tracesAscending(board)) {
        Polyline polyline = trace.polyline();
        if (!trace.isOnTheBoard() || polyline.cornerCount() < 3) {
          continue;
        }
        Point middle = polyline.corner(polyline.cornerCount() / 2);
        if (middle instanceof IntPoint) {
          trace.split(middle);
        }
      }
      out.line(
          new Boards.Line()
              .num("maxGeneratedId", board.communication.idGenerator.maxGeneratedId())
              .num("items", board.getItems().size())
              .end());
    }
    TraceTightener tightener =
        TraceTightener.getInstance(board, new int[0], null, ACCURACY, null, -1, null, -1);
    for (PolylineTrace trace : tracesAscending(board)) {
      tightener.currentLayer = trace.getLayer();
      tightener.currentHalfWidth = trace.getHalfWidth();
      tightener.currentNetNumbers = trace.netNumbers;
      tightener.currentClearanceClassIndex = trace.clearanceClassIndex();
      tightener.contactPins = null;
      Polyline start = tightener.smoothenStartCornerAtTrace(trace);
      Polyline end = tightener.smoothenEndCornerAtTrace(trace);
      if (start == null && end == null) {
        continue;
      }
      out.line(
          new Boards.Line()
              .num("trace", trace.getId())
              .str("start", start == null ? "-" : corners(start))
              .str("end", end == null ? "-" : corners(end))
              .end());
    }
  }

  // ---- board-level modes ----

  static void boardMode(Boards.Out out, String[] args, AngleRestriction angle, boolean tree)
      throws Exception {
    RoutingBoard board = unfixed(args, angle);
    String stage = "board-" + angleName(angle);
    out.line(new Boards.Line().str("stage", stage).end());
    long startUnits = WorkMeter.current().units();
    long start = System.nanoTime();
    board.startMarkingChangedArea();
    board.markAllChangedArea();
    board.optChangedArea(new int[0], null, ACCURACY, null, null, 0);
    long nanos = System.nanoTime() - start;
    System.err.println(
        "TIMING " + Boards.board(args[0]) + " " + stage + " ms=" + nanos / 1_000_000
            + " units=" + (WorkMeter.current().units() - startUnits));
    dump(out, board);
    if (tree) {
      TreeHash.hash(out, stage + "-tree", board.searchTreeManager.getDefaultTree());
    }
  }

  static boolean pinAt(PolylineTrace trace) {
    for (Collection<Item> contacts : List.of(trace.getStartContacts(), trace.getEndContacts())) {
      for (Item item : contacts) {
        if (item instanceof Pin) {
          return true;
        }
      }
    }
    return false;
  }

  static void pinExit(Boards.Out out, String[] args) throws Exception {
    RoutingBoard board = unfixed(args, AngleRestriction.FORTYFIVE_DEGREE);
    int dist = 4 * board.getMinTraceHalfWidth();
    board.rules.setPinEdgeToTurnDist(dist);
    out.line(new Boards.Line().str("stage", "pin-exit").num("pinEdgeToTurnDist", dist).end());
    List<PolylineTrace> traces = tracesAscending(board);
    List<PolylineTrace> atPins = new ArrayList<>();
    for (int i = traces.size() - 1; i >= 0; i--) {
      if (pinAt(traces.get(i))) {
        atPins.add(traces.get(i));
      }
    }
    for (PolylineTrace trace : atPins) {
      if (!trace.isOnTheBoard()) {
        continue;
      }
      boolean result = trace.pullTight(true, ACCURACY, null);
      out.line(
          new Boards.Line()
              .num("trace", trace.getId())
              .bool("result", result)
              .num("maxGeneratedId", board.communication.idGenerator.maxGeneratedId())
              .end());
    }
    dump(out, board);
  }

  static void remove(Boards.Out out, String[] args, int width) throws Exception {
    RoutingBoard board = unfixed(args, AngleRestriction.FORTYFIVE_DEGREE);
    String name = width == Integer.MAX_VALUE ? "max" : Integer.toString(width);
    out.line(new Boards.Line().str("stage", "remove-" + name).end());
    List<PolylineTrace> traces = tracesAscending(board);
    Random random = new Random(20261017L);
    List<Item> chosen = new ArrayList<>();
    while (chosen.size() < 10 && chosen.size() < traces.size()) {
      PolylineTrace trace = traces.get(random.nextInt(traces.size()));
      if (!chosen.contains(trace)) {
        chosen.add(trace);
      }
    }
    StringBuilder ids = new StringBuilder();
    for (Item item : chosen) {
      ids.append(ids.length() > 0 ? "," : "").append(item.getId());
    }
    boolean result = board.removeItemsAndPullTight(chosen, width, ACCURACY);
    out.line(new Boards.Line().str("removed", ids.toString()).bool("result", result).end());
    dump(out, board);
  }

  /** Records the work meter at every stop check of the tightener. */
  static final class Recorder implements Stoppable {
    long lastUnits = -1;
    int checks;

    @Override
    public void requestStop() {}

    @Override
    public boolean isStopRequested() {
      lastUnits = WorkMeter.current().units();
      checks++;
      return false;
    }
  }

  static void limit(Boards.Out out, String[] args) throws Exception {
    RoutingBoard board = unfixed(args, AngleRestriction.FORTYFIVE_DEGREE);
    out.line(new Boards.Line().str("stage", "limit").num("limitMs", LIMIT_MS).end());
    board.startMarkingChangedArea();
    board.markAllChangedArea();
    // RoutingBoardOperations.optChangedArea with a recording stop token
    Recorder recorder = new Recorder();
    long startUnits = WorkMeter.current().units();
    TraceTightener tightener =
        TraceTightener.getInstance(board, new int[0], null, ACCURACY, recorder, LIMIT_MS, null, 0);
    tightener.optChangedArea(null);
    board.changedArea = null;
    long used = recorder.lastUnits - startUnits;
    out.line(
        new Boards.Line()
            .num("checks", recorder.checks)
            .num("unitsAtLastCheck", used)
            .bool("exceeded", used > LIMIT_MS * 64_000L)
            .end());
    dump(out, board);
  }

  /** The default tree's fingerprint line, as {@code TreeOpsProbe.hash} writes it. */
  static final class TreeHash {
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
