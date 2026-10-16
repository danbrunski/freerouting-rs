import app.freerouting.Freerouting;
import app.freerouting.board.actions.ItemSelectionFilter;
import app.freerouting.board.actions.ItemSelectionFilter.SelectableChoices;
import app.freerouting.board.facade.RoutingBoard;
import app.freerouting.board.model.items.ConductionArea;
import app.freerouting.board.model.items.DrillItem;
import app.freerouting.board.model.items.Item;
import app.freerouting.board.model.items.Pin;
import app.freerouting.board.model.structure.FixedState;
import app.freerouting.board.searchtree.ShapeSearchTree;
import app.freerouting.board.searchtree.ShapeSearchTree45Degree;
import app.freerouting.board.searchtree.ShapeSearchTree90Degree;
import app.freerouting.board.trace.PolylineTrace;
import app.freerouting.core.RoutingJob;
import app.freerouting.geometry.planar.FloatPoint;
import app.freerouting.geometry.planar.IntBox;
import app.freerouting.geometry.planar.IntOctagon;
import app.freerouting.geometry.planar.IntPoint;
import app.freerouting.geometry.planar.IntVector;
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
import java.util.Map;
import java.util.Random;
import java.util.Set;
import java.util.TreeSet;
import java.util.UUID;

/**
 * Board probe {@code board-queries} (B04.1): the read-only board checks and picks the router, the
 * shover and pull-tight call (B04.2-B04.7), on seeded samples.
 *
 * <p>Boards ({@link #PLAN}): Issue026-J2_reference with its SES (stage {@code ses}, then a fresh
 * load with clearance compensation on, stage {@code compensated}), Issue508-DAC2020_bm08 with
 * {@code Issue508-DAC2020_bm08-routed.ses} (stage {@code ses}), and Issue413-test,
 * Issue957-smoke-ldo-led, Issue145-smoothieboard and Issue676-ch32v-tx118s loaded only (stage
 * {@code load}). Every other fixtures.tsv row is loaded too: the first row (in fixtures.tsv order)
 * whose board has a pin of more than one net is probed with the row's session (stage {@code ses},
 * {@code load} without one); the probe exits 3 on the rest. The session column of fixtures.tsv
 * is used for that board only.
 *
 * <p>Sampler, per stage: {@code new Random(20261015)}; for each of {@value #SAMPLES} samples, in
 * this order: {@code x = ll.x + nextInt(width + 1)}, {@code y = ll.y + nextInt(height + 1)} (the
 * board bounding box), {@code layer = nextInt(layerCount)}, {@code net = nextInt(maxNetNumber) + 1}
 * (0 without drawing when there are no nets), {@code hw = 1 + nextInt(2 * maxTraceHalfWidth)},
 * {@code cl = nextInt(clearanceClassCount)}, then with {@code r = min(width, height) / 10}: {@code
 * x2 = x - r + nextInt(2 * r + 1)}, {@code y2 = y - r + nextInt(2 * r + 1)}. Pin pairs: pins in
 * item-list order, pairs (i, j > i) in nested order, at most {@value #MAX_PAIRS}: the first net of
 * the first pin the second pin contains, the first common signal layer, the net's trace half width
 * on it and its net class's trace clearance class, the segment between the centres (pairs with
 * equal centres are skipped).
 *
 * <p>Sections (each starts with a {@code {"stage":"<stage>:<method>"}} line, so a digest has one
 * line per method): {@code inputs}, {@code checkShape}, {@code checkTraceShape}, {@code
 * checkPolylineTrace}, {@code checkTraceSegment}, {@code pickItems}, {@code
 * pickNearestRoutingItem}, {@code checkMoveItem}, {@code checkChangeNet}, {@code items}, and for
 * Issue026-J2_reference stage {@code ses} also {@code offsetShapes}. Item ids are listed in Java's
 * {@code TreeSet<Item>} order (highest first); doubles are raw bits; shapes are written by {@link
 * #shape}.
 *
 * <pre>
 * tools/java-reference/probe.sh &lt;pin&gt; tools/java-reference/probes/board/BoardQueriesProbe.java \
 *     board.dsn - out.jsonl
 * </pre>
 */
public class BoardQueriesProbe {
  static final long SEED = 20261015L;
  static final int SAMPLES = 300;
  static final int MAX_PAIRS = 200;
  static final int OFFSET_POLYLINES = 200;
  /** The id of the probe's temporary traces, so that building one takes no id. */
  static final int TEMP_ID = Integer.MAX_VALUE;

  static final String J2 = "Issue026-J2_reference";
  /** Board -> session below the repository root, or "-" for load only. */
  static final Map<String, String> PLAN =
      Map.of(
          J2, "fixtures/Issue026-J2_reference.ses",
          "Issue508-DAC2020_bm08", "fixtures/Issue508-DAC2020_bm08-routed.ses",
          "Issue413-test", "-",
          "Issue957-smoke-ldo-led", "-",
          "Issue145-smoothieboard", "-",
          "Issue676-ch32v-tx118s", "-");

  public static void main(String[] args) throws Exception {
    String board = Boards.board(args[0]);
    String ses = PLAN.get(board);
    if (ses == null) {
      if (!isFirstTiePinBoard(args[0])) {
        System.exit(3);
      }
      // The row's session, so that traces of the tie pin's nets reach it.
      ses = args[1];
    }
    Boards.Out out = new Boards.Out("board-queries", args[0]);
    try {
      RoutingBoard routingBoard = Boards.load(args[0]);
      if (!ses.equals("-")) {
        Boards.importSession(routingBoard, ses);
      }
      runStage(out, routingBoard, ses.equals("-") ? "load" : "ses", board.equals(J2));
      if (board.equals(J2)) {
        RoutingBoard compensated = Boards.load(args[0]);
        Boards.importSession(compensated, ses);
        compensated.searchTreeManager.setClearanceCompensationUsed(true);
        runStage(out, compensated, "compensated", false);
      }
    } catch (Throwable t) {
      out.exception(t);
    }
    out.write(args[2]);
  }

  /** The pins with more than one net, in item-list order. */
  static List<Item> multiNetPins(RoutingBoard board) {
    List<Item> result = new ArrayList<>();
    for (Pin pin : board.getPins()) {
      if (pin.netCount() > 1) {
        result.add(pin);
      }
    }
    return result;
  }

  /** True if {@code dsn} has a multi-net pin and no earlier fixtures.tsv row has one. */
  static boolean isFirstTiePinBoard(String dsn) throws Exception {
    if (multiNetPins(Boards.load(dsn)).isEmpty()) {
      return false;
    }
    String own = Boards.board(dsn);
    Path rows = Path.of("tools/java-reference/probes/board/fixtures.tsv");
    for (String row : Files.readAllLines(rows, StandardCharsets.UTF_8)) {
      if (row.isEmpty() || row.startsWith("#")) {
        continue;
      }
      String rowDsn = row.split("\t")[0];
      if (Boards.board(rowDsn).equals(own)) {
        return true;
      }
      try {
        if (!multiNetPins(Boards.load(rowDsn)).isEmpty()) {
          return false;
        }
      } catch (Exception e) {
        // A board Java cannot load has no pins.
      }
    }
    return true;
  }

  record Sample(int x, int y, int layer, int net, int hw, int cl, int x2, int y2) {
    Point from() {
      return new IntPoint(x, y);
    }

    Point to() {
      return new IntPoint(x2, y2);
    }
  }

  record Pair(Pin a, Pin b, int layer, int net, int hw, int cl) {}

  static void runStage(Boards.Out out, RoutingBoard board, String stage, boolean offsetShapes) {
    IntBox box = board.boundingBox;
    int width = box.ur.x - box.ll.x;
    int height = box.ur.y - box.ll.y;
    int layers = board.getLayerCount();
    int maxNet = board.rules.nets.maxNetNumber();
    int maxHalfWidth = board.getMaxTraceHalfWidth();
    int classes = board.rules.clearanceMatrix.getClassCount();
    Random random = new Random(SEED);
    List<Sample> samples = new ArrayList<>();
    int r = Math.min(width, height) / 10;
    for (int i = 0; i < SAMPLES; i++) {
      int x = box.ll.x + random.nextInt(width + 1);
      int y = box.ll.y + random.nextInt(height + 1);
      int layer = random.nextInt(layers);
      int net = maxNet > 0 ? random.nextInt(maxNet) + 1 : 0;
      int hw = 1 + random.nextInt(2 * maxHalfWidth);
      int cl = random.nextInt(classes);
      int x2 = x - r + random.nextInt(2 * r + 1);
      int y2 = y - r + random.nextInt(2 * r + 1);
      samples.add(new Sample(x, y, layer, net, hw, cl, x2, y2));
    }
    List<Pair> pairs = pinPairs(board);

    section(out, board, stage, "inputs")
        .num("layers", layers)
        .num("maxNetNo", maxNet)
        .num("maxTraceHalfWidth", maxHalfWidth)
        .num("classes", classes)
        .raw("box", "[" + box.ll.x + "," + box.ll.y + "," + box.ur.x + "," + box.ur.y + "]")
        .bool("compensated", board.searchTreeManager.isClearanceCompensationUsed())
        .raw("multiNetPins", ids(multiNetPins(board)))
        .num("samples", samples.size())
        .num("pairs", pairs.size());
    flush(out);
    for (int i = 0; i < samples.size(); i++) {
      Sample s = samples.get(i);
      out.line(
          new Boards.Line()
              .num("i", i)
              .num("x", s.x)
              .num("y", s.y)
              .num("layer", s.layer)
              .num("net", s.net)
              .num("hw", s.hw)
              .num("cl", s.cl)
              .num("x2", s.x2)
              .num("y2", s.y2)
              .end());
    }
    for (int k = 0; k < pairs.size(); k++) {
      Pair p = pairs.get(k);
      out.line(
          new Boards.Line()
              .num("pair", k)
              .num("a", p.a.getId())
              .num("b", p.b.getId())
              .num("layer", p.layer)
              .num("net", p.net)
              .num("hw", p.hw)
              .num("cl", p.cl)
              .end());
    }

    // checkShape: the octagon around the sample point, enlarged by the half width.
    section(out, board, stage, "checkShape");
    flush(out);
    for (int i = 0; i < samples.size(); i++) {
      Sample s = samples.get(i);
      Boards.Line line = new Boards.Line().num("i", i);
      try {
        IntOctagon octagon = s.from().surroundingOctagon().enlarge(s.hw);
        line.bool("ok", board.checkShape(octagon, s.layer, new int[] {s.net}, s.cl));
      } catch (Exception e) {
        line.str("exception", e.toString());
      }
      out.line(line.end());
    }

    // The segments: the random ones, then the pin pairs.
    List<Segment> segments = new ArrayList<>();
    for (int i = 0; i < samples.size(); i++) {
      Sample s = samples.get(i);
      segments.add(new Segment("i", i, s.from(), s.to(), s.layer, s.net, s.hw, s.cl));
    }
    for (int k = 0; k < pairs.size(); k++) {
      Pair p = pairs.get(k);
      segments.add(
          new Segment("pair", k, p.a.getCenter(), p.b.getCenter(), p.layer, p.net, p.hw, p.cl));
    }

    section(out, board, stage, "checkTraceShape");
    flush(out);
    for (Segment g : segments) {
      Boards.Line line = new Boards.Line().num(g.key, g.index);
      if (g.from.equals(g.to)) {
        out.line(line.bool("skip", true).end());
        continue;
      }
      try {
        Polyline polyline = new Polyline(g.from, g.to);
        TileShape shape = polyline.offsetShape(g.hw, 0);
        PolylineTrace trace =
            new PolylineTrace(
                polyline, g.layer, g.hw, g.nets(), g.cl, TEMP_ID, 0, FixedState.UNFIXED, board);
        Set<Pin> pins = trace.touchingPinsAtEndCorners();
        line.raw("pins", ids(pins));
        line.bool("ok", board.checkTraceShape(shape, g.layer, g.nets(), g.cl, pins));
      } catch (Exception e) {
        line.str("exception", e.toString());
      }
      out.line(line.end());
    }

    section(out, board, stage, "checkPolylineTrace");
    flush(out);
    for (Segment g : segments) {
      Boards.Line line = new Boards.Line().num(g.key, g.index);
      if (g.from.equals(g.to)) {
        out.line(line.bool("skip", true).end());
        continue;
      }
      try {
        Polyline polyline = new Polyline(g.from, g.to);
        line.bool("ok", board.checkPolylineTrace(polyline, g.layer, g.hw, g.nets(), g.cl));
      } catch (Exception e) {
        line.str("exception", e.toString());
      }
      line.num("maxGeneratedId", board.communication.idGenerator.maxGeneratedId());
      out.line(line.end());
    }

    section(out, board, stage, "checkTraceSegment");
    flush(out);
    for (Segment g : segments) {
      Boards.Line line = new Boards.Line().num(g.key, g.index);
      try {
        StringBuilder point = new StringBuilder("[");
        for (boolean only : new boolean[] {false, true}) {
          double value =
              board.checkTraceSegment(g.from, g.to, g.layer, g.nets(), g.hw, g.cl, only);
          point.append(only ? "," : "").append('"').append(Boards.bits(value)).append('"');
        }
        line.raw("point", point.append(']').toString());
        if (!g.from.equals(g.to)) {
          LineSegment segment = new LineSegment(new Polyline(g.from, g.to), 1);
          StringBuilder seg = new StringBuilder("[");
          for (boolean only : new boolean[] {false, true}) {
            double value =
                board.checkTraceSegment(segment, g.layer, g.nets(), g.hw, g.cl, only);
            seg.append(only ? "," : "").append('"').append(Boards.bits(value)).append('"');
          }
          line.raw("segment", seg.append(']').toString());
        }
      } catch (Exception e) {
        line.str("exception", e.toString());
      }
      out.line(line.end());
    }

    // The pick points: the random samples, then the first pin of each pair on the pair's layer.
    List<Segment> picks = new ArrayList<>(segments);

    section(out, board, stage, "pickItems");
    flush(out);
    SelectableChoices[] choices = SelectableChoices.values();
    for (Segment g : picks) {
      Boards.Line line = new Boards.Line().num(g.key, g.index);
      line.raw("none", ids(board.pickItems(g.from, g.layer, null)));
      for (SelectableChoices choice : choices) {
        line.raw(choice.name(), ids(board.pickItems(g.from, g.layer, new ItemSelectionFilter(choice))));
      }
      line.raw("default", ids(board.pickItems(g.from, g.layer, new ItemSelectionFilter())));
      out.line(line.end());
    }

    section(out, board, stage, "pickNearestRoutingItem");
    flush(out);
    for (Segment g : picks) {
      Boards.Line line = new Boards.Line().num(g.key, g.index);
      Item nearest = board.pickNearestRoutingItem(g.from, g.layer, null);
      line.num("nearest", nearest == null ? -1 : nearest.getId());
      if (nearest != null) {
        Item from = board.pickNearestRoutingItem(g.from, g.layer, nearest);
        line.num("from", from == null ? -1 : from.getId());
      }
      out.line(line.end());
    }

    section(out, board, stage, "checkMoveItem");
    flush(out);
    for (Segment g : picks) {
      Boards.Line line = new Boards.Line().num(g.key, g.index);
      Item item = highestPick(board, g);
      line.num("item", item == null ? -1 : item.getId());
      if (item != null) {
        IntPoint a = (IntPoint) g.from;
        IntPoint b = (IntPoint) g.to;
        IntVector vector = new IntVector(b.x - a.x, b.y - a.y);
        try {
          line.bool("plain", board.checkMoveItem(item, vector, null));
          Set<Item> ignore = new TreeSet<>();
          line.bool("ignore", board.checkMoveItem(item, vector, ignore));
          line.raw("ignored", ids(ignore));
        } catch (Exception e) {
          line.str("exception", e.toString());
        }
      }
      out.line(line.end());
    }

    section(out, board, stage, "checkChangeNet");
    flush(out);
    for (Segment g : picks) {
      Boards.Line line = new Boards.Line().num(g.key, g.index);
      Item item = highestPick(board, g);
      line.num("item", item == null ? -1 : item.getId());
      if (item != null) {
        try {
          line.bool("ok", board.checkChangeNet(item, g.net));
        } catch (Exception e) {
          line.str("exception", e.toString());
        }
      }
      out.line(line.end());
    }

    section(out, board, stage, "items");
    flush(out);
    ShapeSearchTree tree = board.searchTreeManager.getDefaultTree();
    for (Item item : board.getItems()) {
      Boards.Line line = new Boards.Line().num("item", item.getId());
      if (item instanceof DrillItem drill) {
        line.str("type", item.getClass().getSimpleName());
        line.dbl("minWidth", drill.minWidth());
        line.str("conn", shape(drill.getTraceConnectionShape(tree, 0)));
        List<String> tiles = new ArrayList<>();
        for (int l = drill.firstLayer(); l <= drill.lastLayer(); l++) {
          tiles.add(shape(drill.getTileShapeOnLayer(l)));
        }
        line.raw("tiles", strings(tiles));
        if (item instanceof Pin pin) {
          line.bool("drillAllowed", pin.drillAllowed());
        }
      } else if (item instanceof PolylineTrace trace) {
        line.str("type", "PolylineTrace");
        line.num("compensatedHalfWidth", trace.getCompensatedHalfWidth(tree));
        line.str("nearestLl", point(trace.nearestEndPoint(box.ll)));
        line.str("nearestUr", point(trace.nearestEndPoint(box.ur)));
        line.raw("touchingPins", ids(trace.touchingPinsAtEndCorners()));
        List<String> conn = new ArrayList<>();
        for (int i = 0; i < trace.tileShapeCount(); i++) {
          conn.add(shape(trace.getTraceConnectionShape(tree, i)));
        }
        line.raw("conn", strings(conn));
      } else if (item instanceof ConductionArea area) {
        line.str("type", "ConductionArea");
        List<String> conn = new ArrayList<>();
        for (int i = 0; i < area.treeShapeCount(tree); i++) {
          conn.add(shape(area.getTraceConnectionShape(tree, i)));
        }
        line.raw("conn", strings(conn));
      } else {
        continue;
      }
      StringBuilder connected = new StringBuilder("[");
      for (int l = 0; l < layers; l++) {
        if (item.isConnectedOnLayer(l)) {
          connected.append(connected.length() > 1 ? "," : "").append(l);
        }
      }
      line.raw("connectedOnLayers", connected.append(']').toString());
      out.line(line.end());
    }

    if (offsetShapes) {
      offsetShapes(out, board, stage);
    }
  }

  /** The highest-id item under a pick point (the first of Java's TreeSet), or null. */
  static Item highestPick(RoutingBoard board, Segment g) {
    Set<Item> picked = board.pickItems(g.from, g.layer, null);
    return picked.isEmpty() ? null : picked.iterator().next();
  }

  /**
   * {@code offsetShapes} of the three tree kinds on {@value #OFFSET_POLYLINES} seeded polylines:
   * {@code new Random(SEED)}; per polyline {@code corners = 2 + nextInt(4)}, the first corner drawn
   * like a sample point, each next one like a sample's second point around the previous corner;
   * then {@code hw = 1 + nextInt(2 * maxTraceHalfWidth)}, {@code n} = the line count of {@code new
   * Polyline(corners)}, {@code from = nextInt(n + 2) - 1}, {@code to = from + nextInt(n + 2)}.
   */
  static void offsetShapes(Boards.Out out, RoutingBoard board, String stage) {
    section(out, board, stage, "offsetShapes");
    flush(out);
    IntBox box = board.boundingBox;
    int width = box.ur.x - box.ll.x;
    int height = box.ur.y - box.ll.y;
    int r = Math.min(width, height) / 10;
    int maxHalfWidth = board.getMaxTraceHalfWidth();
    ShapeSearchTree plain = board.searchTreeManager.getDefaultTree();
    ShapeSearchTree deg45 = new ShapeSearchTree45Degree(board, 0);
    ShapeSearchTree deg90 = new ShapeSearchTree90Degree(board, 0);
    Random random = new Random(SEED);
    for (int k = 0; k < OFFSET_POLYLINES; k++) {
      int count = 2 + random.nextInt(4);
      Point[] corners = new Point[count];
      int x = box.ll.x + random.nextInt(width + 1);
      int y = box.ll.y + random.nextInt(height + 1);
      StringBuilder echo = new StringBuilder("[");
      for (int c = 0; c < count; c++) {
        if (c > 0) {
          x = x - r + random.nextInt(2 * r + 1);
          y = y - r + random.nextInt(2 * r + 1);
          echo.append(',');
        }
        corners[c] = new IntPoint(x, y);
        echo.append(x).append(',').append(y);
      }
      int hw = 1 + random.nextInt(2 * maxHalfWidth);
      Polyline polyline = new Polyline(corners);
      int n = polyline.lines.length;
      int from = random.nextInt(n + 2) - 1;
      int to = from + random.nextInt(n + 2);
      Boards.Line line =
          new Boards.Line()
              .num("k", k)
              .raw("corners", echo.append(']').toString())
              .num("hw", hw)
              .num("lines", n)
              .num("from", from)
              .num("to", to);
      line.raw("plain", shapes(plain.offsetShapes(polyline, hw, from, to)));
      line.raw("deg45", shapes(deg45.offsetShapes(polyline, hw, from, to)));
      line.raw("deg90", shapes(deg90.offsetShapes(polyline, hw, from, to)));
      out.line(line.end());
    }
  }

  record Segment(
      String key, int index, Point from, Point to, int layer, int net, int hw, int cl) {
    int[] nets() {
      return new int[] {net};
    }
  }

  static List<Pair> pinPairs(RoutingBoard board) {
    List<Pin> pins = new ArrayList<>(board.getPins());
    List<Pair> result = new ArrayList<>();
    for (int i = 0; i < pins.size(); i++) {
      Pin a = pins.get(i);
      for (int j = i + 1; j < pins.size(); j++) {
        Pin b = pins.get(j);
        int net = -1;
        for (int n : a.netNumbers) {
          if (b.containsNet(n)) {
            net = n;
            break;
          }
        }
        if (net < 0) {
          continue;
        }
        int layer = -1;
        int last = Math.min(a.lastLayer(), b.lastLayer());
        for (int l = Math.max(a.firstLayer(), b.firstLayer()); l <= last; l++) {
          if (board.layerStructure.layers[l].isSignal) {
            layer = l;
            break;
          }
        }
        if (layer < 0 || a.getCenter().equals(b.getCenter())) {
          continue;
        }
        int hw = board.rules.getTraceHalfWidth(net, layer);
        int cl = board.rules.nets.get(net).getNetClass().getTraceClearanceClass();
        result.add(new Pair(a, b, layer, net, hw, cl));
        if (result.size() >= MAX_PAIRS) {
          return result;
        }
      }
    }
    return result;
  }

  private static Boards.Line pending;

  /** Starts a section line; {@link #flush} writes it after the caller added its fields. */
  static Boards.Line section(Boards.Out out, RoutingBoard board, String stage, String method) {
    pending =
        new Boards.Line()
            .str("stage", stage + ":" + method)
            .num("maxGeneratedId", board.communication.idGenerator.maxGeneratedId());
    return pending;
  }

  static void flush(Boards.Out out) {
    out.line(pending.end());
    pending = null;
  }

  static String ids(Collection<? extends Item> items) {
    StringBuilder text = new StringBuilder("[");
    for (Item item : items) {
      text.append(text.length() > 1 ? "," : "").append(item.getId());
    }
    return text.append(']').toString();
  }

  static String strings(List<String> values) {
    StringBuilder text = new StringBuilder("[");
    for (String value : values) {
      text.append(text.length() > 1 ? "," : "").append('"').append(value).append('"');
    }
    return text.append(']').toString();
  }

  static String shapes(TileShape[] values) {
    List<String> text = new ArrayList<>();
    for (TileShape value : values) {
      text.add(shape(value));
    }
    return strings(text);
  }

  /** An integer point as {@code x,y}; any other point as {@code f<x bits>,<y bits>}. */
  static String point(Point p) {
    if (p instanceof IntPoint ip) {
      return ip.x + "," + ip.y;
    }
    FloatPoint f = p.toFloat();
    return "f" + Boards.bits(f.x) + "," + Boards.bits(f.y);
  }

  /**
   * A tile shape: {@code box llx lly urx ury}, {@code oct} and the eight octagon values (left x,
   * bottom y, right x, top y, upper left, lower right, lower left, upper right diagonal x), or
   * {@code simplex} and the two points of each border line; {@code null} for none.
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
