import app.freerouting.Freerouting;
import app.freerouting.board.actions.ItemSelectionFilter;
import app.freerouting.board.facade.RoutingBoard;
import app.freerouting.board.model.items.ConductionArea;
import app.freerouting.board.model.items.DrillItem;
import app.freerouting.board.model.items.Item;
import app.freerouting.board.model.items.Pin;
import app.freerouting.board.model.items.Trace;
import app.freerouting.board.model.items.Via;
import app.freerouting.board.trace.PolylineTrace;
import app.freerouting.core.RoutingJob;
import app.freerouting.geometry.planar.FloatPoint;
import app.freerouting.geometry.planar.IntPoint;
import app.freerouting.geometry.planar.Point;
import app.freerouting.geometry.planar.Polyline;
import app.freerouting.gui.interactive.Route;
import app.freerouting.io.specctra.SesReader;
import app.freerouting.management.BoardLoader;
import app.freerouting.rules.Net;
import app.freerouting.rules.NetClass;
import app.freerouting.rules.ViaRule;
import app.freerouting.settings.GlobalSettings;
import java.io.FileInputStream;
import java.io.InputStream;
import java.lang.reflect.Field;
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
 * Board probe {@code route-script} (I01.7): interactive routing driven headless through {@code
 * gui.interactive.Route}, mirrored by {@code crates/freerouting/tests/interactive_route.rs}.
 *
 * <p>{@code Route} needs only a {@code RoutingBoard}; its {@code draw} is never called. The route
 * states need a {@code GuiBoardManager} (Swing), so this probe transcribes their board calls:
 * {@code RouteState.getInstance} (with {@code startOk}, {@code pickRoutingItem}, {@code
 * getRouteNetNumbersAtTiePin} and {@code GuiBoardRoutingSettings}' rule lookups), {@code
 * changeLayerAction}, {@code cancel}, and {@code StitchRouteState.addCorner}'s snapshot. The
 * settings are fixed: the current layer is 0 at each route start, select on all visible layers, all
 * layers visible, no manual rules, tidy width {@code Integer.MAX_VALUE}, accuracy 500, neckdown and
 * via snap to SMD centres on, obstacle highlight off. {@code Route}'s work limits are its own (3000
 * nominal ms per check, 2000 for pull-tight).
 *
 * <p>Boards: the ten of {@link #BOARDS}, stage {@code load} as loaded, J2 and DAC2020_bm08 also
 * stage {@code ses} with their fixtures.tsv session; every other row exits 3. Each stage runs four
 * variants on a fresh board: push on and off, each dynamic and stitch ({@code stitch} on). Per
 * variant one {@code new Random(SEED)} draws {@value #PIN_STARTS} start pins (pins with a net,
 * highest id first) and then {@value #TRACE_STARTS} trace starts (the middle of the first segment
 * of a trace, traces highest id first). For each started route the target is the target pin
 * nearest to the start (the first of equals in target order); the route steps to {@code start +
 * (target - start) * k / 12} for k = 1..8, changes to the next signal layer after the current one
 * (else the previous one), and steps k = 9..12; it stops when a step or the layer change ends it.
 * Every fifth route that does not end itself is cancelled ({@code RouteState.cancel}). A stitch
 * step takes a snapshot first.
 *
 * <p>Lines: each start, step and layer change starts with {@code "stage"} (stage/variant), then the
 * route number, the outcome, the route's and the current layer, the last corner, the shove-failing
 * obstacle and the nearest target item ({@code Route}'s private fields, -1 for none) and {@code
 * maxGeneratedId}; the canonical item dump follows at the end of each route.
 */
public class RouteScriptProbe {
  static final long SEED = 20261016L;
  static final int PIN_STARTS = 20;
  static final int TRACE_STARTS = 4;
  static final Map<String, Boolean> BOARDS =
      Map.of(
          "Issue026-J2_reference", true,
          "Issue508-DAC2020_bm08", true,
          "Issue508-DAC2020_bm05", false,
          "Issue143-rpi_splitter", false,
          "Issue413-test", false,
          "Issue676-ch32v-tx118s", false,
          "Issue957-smoke-ldo-led", false,
          "Issue508-SMD-routing-issue-demo", false,
          "scoring-mixed-layer", false,
          "Issue110-testProjectFromFreeroutingBugTest", false);

  public static void main(String[] args) throws Exception {
    String board = Boards.board(args[0]);
    Boolean withSession = BOARDS.get(board);
    if (withSession == null) {
      System.exit(3);
    }
    Boards.Out out = new Boards.Out("route-script", args[0]);
    try {
      long started = System.nanoTime();
      List<String> stages = new ArrayList<>(List.of("load"));
      if (withSession) {
        stages.add("ses");
      }
      for (String stage : stages) {
        for (String variant : new String[] {"push-dynamic", "nopush-dynamic", "push-stitch", "nopush-stitch"}) {
          RoutingBoard routingBoard = Boards.load(args[0]);
          if (stage.equals("ses")) {
            Boards.importSession(routingBoard, args[1]);
          }
          Ws ws = new Ws();
          ws.push = variant.startsWith("push");
          ws.stitch = variant.endsWith("stitch");
          script(out, routingBoard, stage + "/" + variant, ws);
        }
      }
      System.err.printf("route-script: %.1f ms%n", (System.nanoTime() - started) / 1e6);
    } catch (Throwable t) {
      out.exception(t);
    }
    out.write(args[2]);
  }

  /** The workspace settings the probe fixes (Java's {@code WorkspaceSettings} defaults). */
  static final class Ws {
    int layer = 0;
    boolean push = true;
    boolean stitch = false;
    int tidy = Integer.MAX_VALUE;
    int accuracy = 500;
    boolean neckdown = true;
    boolean viaSnap = true;
    boolean highlight = false;
  }

  /** A started route and what the route state keeps beside it. */
  static final class Started {
    Route route;
    int[] nets;
    Set<Item> targets;
  }

  static Object field(Route route, String name) throws Exception {
    Field f = Route.class.getDeclaredField(name);
    f.setAccessible(true);
    return f.get(route);
  }

  static long id(Object item) {
    return item == null ? -1 : ((Item) item).getId();
  }

  static Boards.Line state(Boards.Line line, RoutingBoard board, Route route, Ws ws) throws Exception {
    return line.num("layer", (Integer) field(route, "layer"))
        .num("current", ws.layer)
        .str("lastCorner", point(route.getLastCorner()))
        .num("failing", id(field(route, "shoveFailingObstacle")))
        .num("nearest", id(field(route, "nearestTargetItem")))
        .num("maxGeneratedId", board.communication.idGenerator.maxGeneratedId());
  }

  static List<Item> sortedItems(RoutingBoard board) {
    List<Item> items = new ArrayList<>(board.getItems());
    items.sort((a, b) -> Integer.compare(b.getId(), a.getId()));
    return items;
  }

  static void script(Boards.Out out, RoutingBoard board, String stage, Ws ws) throws Exception {
    Random random = new Random(SEED);
    List<Pin> pins = new ArrayList<>();
    for (Item item : sortedItems(board)) {
      if (item instanceof Pin pin && pin.netCount() > 0) {
        pins.add(pin);
      }
    }
    for (int r = 0; r < PIN_STARTS + TRACE_STARTS; r++) {
      FloatPoint at;
      if (r < PIN_STARTS) {
        if (pins.isEmpty()) {
          out.line(new Boards.Line().str("stage", stage).num("route", r).str("skip", "no pins").end());
          continue;
        }
        at = pins.get(random.nextInt(pins.size())).getCenter().toFloat();
      } else {
        List<PolylineTrace> traces = new ArrayList<>();
        for (Item item : sortedItems(board)) {
          if (item instanceof PolylineTrace trace) {
            traces.add(trace);
          }
        }
        if (traces.isEmpty()) {
          out.line(new Boards.Line().str("stage", stage).num("route", r).str("skip", "no traces").end());
          continue;
        }
        Polyline polyline = traces.get(random.nextInt(traces.size())).polyline();
        at = polyline.cornerApprox(0).middlePoint(polyline.cornerApprox(1));
      }
      route(out, board, stage, ws, r, at);
    }
  }

  static void route(Boards.Out out, RoutingBoard board, String stage, Ws ws, int r, FloatPoint at)
      throws Exception {
    ws.layer = 0;
    Boards.Line line = new Boards.Line().str("stage", stage).num("route", r).str("at", fpoint(at));
    Started started = start(board, at, ws, line);
    if (started == null) {
      out.line(line.num("maxGeneratedId", board.communication.idGenerator.maxGeneratedId()).end());
      return;
    }
    Route route = started.route;
    FloatPoint target = null;
    double minDist = Double.MAX_VALUE;
    FloatPoint from = route.getLastCorner().toFloat();
    for (Item item : started.targets) {
      if (item instanceof Pin pin) {
        FloatPoint c = pin.getCenter().toFloat();
        double d = from.distance(c);
        if (d < minDist) {
          minDist = d;
          target = c;
        }
      }
    }
    out.line(state(line.str("target", target == null ? "-" : fpoint(target)), board, route, ws).end());
    boolean ended = false;
    for (int k = 1; k <= 12 && target != null && !ended; k++) {
      if (k == 9) {
        int to = nextSignalLayer(board, ws.layer);
        String outcome = changeLayer(board, route, started.nets, to, ws);
        ended = outcome.equals("plane");
        out.line(
            state(
                    new Boards.Line()
                        .str("stage", stage)
                        .num("route", r)
                        .str("kind", "layer")
                        .num("to", to)
                        .str("outcome", outcome)
                        .num("failingLayer", board.getShoveFailingLayer()),
                    board,
                    route,
                    ws)
                .end());
        if (ended) {
          break;
        }
      }
      FloatPoint p = new FloatPoint(from.x + (target.x - from.x) * k / 12.0, from.y + (target.y - from.y) * k / 12.0);
      if (ws.stitch || isStitch(board, started.nets)) {
        board.generateSnapshot();
      }
      ended = route.nextCorner(p);
      out.line(
          state(
                  new Boards.Line()
                      .str("stage", stage)
                      .num("route", r)
                      .str("kind", "corner")
                      .num("k", k)
                      .str("at", fpoint(p))
                      .bool("ended", ended),
                  board,
                  route,
                  ws)
              .end());
    }
    if (!ended && r % 5 == 4) {
      cancel(board, route, started.nets, ws);
      out.line(new Boards.Line().str("stage", stage).num("route", r).str("kind", "cancel").num("maxGeneratedId", board.communication.idGenerator.maxGeneratedId()).end());
    }
    dump(out, board);
  }

  /** The next signal layer after {@code layer} (RouteState's '+'), else the previous one ('-'). */
  static int nextSignalLayer(RoutingBoard board, int layer) {
    int count = board.layerStructure.layers.length;
    for (int i = layer + 1; i < count; i++) {
      if (board.layerStructure.layers[i].isSignal) {
        return i;
      }
    }
    for (int i = layer - 1; i >= 0; i--) {
      if (board.layerStructure.layers[i].isSignal) {
        return i;
      }
    }
    return layer;
  }

  static boolean isStitch(RoutingBoard board, int[] nets) {
    NetClass netClass = board.rules.nets.get(nets[0]).getNetClass();
    return netClass.isShoveFixed() || !netClass.getPullTight();
  }

  // ---- RouteState.getInstance, transcribed ----

  static Started start(RoutingBoard board, FloatPoint floatLocation, Ws ws, Boards.Line line) {
    IntPoint location = floatLocation.round();
    Item pickedItem = startOk(board, location, ws);
    if (pickedItem == null) {
      line.str("start", "nothing");
      return null;
    }
    int netCount = pickedItem.netCount();
    if (netCount <= 0) {
      line.str("start", "nonets");
      return null;
    }
    int[] routeNetNoArr;
    if (pickedItem instanceof Pin pin && netCount > 1) {
      routeNetNoArr = getRouteNetNumbersAtTiePin(pin, ws.layer);
    } else {
      routeNetNoArr = new int[netCount];
      for (int i = 0; i < netCount; i++) {
        routeNetNoArr[i] = pickedItem.getNetNumber(i);
      }
    }
    if (routeNetNoArr.length == 0) {
      line.str("start", "nonets");
      return null;
    }
    int[] traceHalfWidths = new int[board.getLayerCount()];
    boolean[] layerActiveArr = new boolean[traceHalfWidths.length];
    for (int i = 0; i < traceHalfWidths.length; i++) {
      traceHalfWidths[i] = board.rules.getTraceHalfWidth(routeNetNoArr[0], i);
      layerActiveArr[i] = false;
      for (int j = 0; j < routeNetNoArr.length; j++) {
        if (isActiveRoutingLayer(board, routeNetNoArr[j], i)) {
          layerActiveArr[i] = true;
        }
      }
    }
    int traceClearanceClass =
        board.rules.nets.get(routeNetNoArr[0]).getNetClass().getTraceClearanceClass();
    boolean startOk = true;
    if (pickedItem instanceof Trace pickedTrace) {
      Point pickedCorner = pickedTrace.nearestEndPoint(location);
      if (pickedCorner instanceof IntPoint point
          && floatLocation.distance(pickedCorner.toFloat()) < 5 * pickedTrace.getHalfWidth()) {
        location = point;
      } else {
        if (pickedTrace instanceof PolylineTrace trace) {
          FloatPoint nearestPoint = trace.polyline().nearestPointApprox(floatLocation);
          location = nearestPoint.round();
        }
        if (!board.connectToTrace(
            location, pickedTrace, pickedTrace.getHalfWidth(), pickedTrace.clearanceClassIndex())) {
          startOk = false;
        }
      }
      if (startOk) {
        int[] newTraceHalfWidths = new int[traceHalfWidths.length];
        System.arraycopy(traceHalfWidths, 0, newTraceHalfWidths, 0, traceHalfWidths.length);
        newTraceHalfWidths[pickedTrace.getLayer()] = pickedTrace.getHalfWidth();
        traceHalfWidths = newTraceHalfWidths;
        traceClearanceClass = pickedTrace.clearanceClassIndex();
      }
    } else if (pickedItem instanceof DrillItem drillItem) {
      Point center = drillItem.getCenter();
      if (center instanceof IntPoint point) {
        location = point;
      }
    }
    if (!startOk) {
      line.str("start", "connect");
      return null;
    }
    Net currentNet = board.rules.nets.get(routeNetNoArr[0]);
    if (currentNet == null) {
      line.str("start", "unknown-net");
      return null;
    }
    boolean isStitchRoute =
        ws.stitch || currentNet.getNetClass().isShoveFixed() || !currentNet.getNetClass().getPullTight();
    board.generateSnapshot();
    Set<Item> targets = pickedItem.getUnconnectedSet(-1);
    ViaRule viaRule = currentNet.getNetClass().getViaRule();
    Started started = new Started();
    started.nets = routeNetNoArr;
    started.targets = targets;
    started.route =
        new Route(
            location, ws.layer, traceHalfWidths, layerActiveArr, routeNetNoArr, traceClearanceClass,
            viaRule, ws.push, ws.tidy, ws.accuracy, pickedItem, targets, board, isStitchRoute,
            ws.neckdown, ws.viaSnap, ws.highlight);
    StringBuilder widths = new StringBuilder();
    for (int i = 0; i < traceHalfWidths.length; i++) {
      widths.append(i > 0 ? "," : "").append(traceHalfWidths[i]);
    }
    StringBuilder nets = new StringBuilder();
    for (int i = 0; i < routeNetNoArr.length; i++) {
      nets.append(i > 0 ? "," : "").append(routeNetNoArr[i]);
    }
    line.str("start", "ok")
        .num("item", pickedItem.getId())
        .str("point", point(location))
        .str("nets", nets.toString())
        .str("halfWidths", widths.toString())
        .num("class", traceClearanceClass)
        .str("viaRule", viaRule == null ? "-" : viaRule.name)
        .bool("stitch", isStitchRoute)
        .num("targets", targets.size());
    return started;
  }

  static boolean isActiveRoutingLayer(RoutingBoard board, int netNumber, int layer) {
    Net currentNet = board.rules.nets.get(netNumber);
    if (currentNet == null) {
      return true;
    }
    NetClass currentNetClass = currentNet.getNetClass();
    return currentNetClass == null || currentNetClass.isActiveRoutingLayer(layer);
  }

  static Item startOk(RoutingBoard board, IntPoint location, Ws ws) {
    Item pickedItem = board.pickNearestRoutingItem(location, ws.layer, null);
    int layerCount = board.getLayerCount();
    if (pickedItem == null) {
      pickedItem = pickRoutingItem(board, location, 0, ws);
      if (pickedItem == null) {
        pickedItem = pickRoutingItem(board, location, layerCount - 1, ws);
      }
      if (pickedItem == null) {
        for (int i = 1; i < layerCount - 1; i++) {
          if (board.layerStructure.layers[i].isSignal) {
            pickedItem = pickRoutingItem(board, location, i, ws);
            if (pickedItem != null) {
              break;
            }
          }
        }
      }
      if (pickedItem == null) {
        for (int i = 1; i < layerCount - 1; i++) {
          if (!board.layerStructure.layers[i].isSignal) {
            pickedItem = pickRoutingItem(board, location, i, ws);
            if (pickedItem != null) {
              break;
            }
          }
        }
      }
    }
    return pickedItem;
  }

  static Item pickRoutingItem(RoutingBoard board, IntPoint location, int layerIndex, Ws ws) {
    if (layerIndex == ws.layer) {
      return null;
    }
    Item pickedItem = board.pickNearestRoutingItem(location, layerIndex, null);
    if (pickedItem != null) {
      ws.layer = pickedItem.firstLayer();
    }
    return pickedItem;
  }

  static int[] getRouteNetNumbersAtTiePin(Pin pin, int layer) {
    Set<Integer> netNumberList = new TreeSet<>();
    for (int i = 0; i < pin.netCount(); i++) {
      netNumberList.add(pin.getNetNumber(i));
    }
    Set<Item> contacts = pin.getNormalContacts();
    for (Item currentContact : contacts) {
      if (currentContact.firstLayer() <= layer && currentContact.lastLayer() >= layer) {
        for (int i = 0; i < currentContact.netCount(); i++) {
          netNumberList.remove(currentContact.getNetNumber(i));
        }
      }
    }
    int[] result = new int[netNumberList.size()];
    int currentInd = 0;
    for (Integer currentNetNumber : netNumberList) {
      result[currentInd] = currentNetNumber;
      ++currentInd;
    }
    return result;
  }

  // ---- RouteState.changeLayerAction and cancel, transcribed ----

  static String changeLayer(RoutingBoard board, Route route, int[] nets, int newLayer, Ws ws) {
    if (newLayer < 0 || newLayer >= board.getLayerCount()) {
      return "ignored";
    }
    boolean changeLayerSucceeded = route.changeLayer(newLayer);
    if (!changeLayerSucceeded) {
      return "failed";
    }
    boolean connectedToPlane = false;
    int oldLayer = ws.layer;
    ItemSelectionFilter selectionFilter =
        new ItemSelectionFilter(ItemSelectionFilter.SelectableChoices.VIAS);
    Collection<Item> pickedItems = board.pickItems(route.getLastCorner(), oldLayer, selectionFilter);
    Via newVia = null;
    for (Item currentVia : pickedItems) {
      if (currentVia.sharesNetNo(nets)) {
        newVia = (Via) currentVia;
        break;
      }
    }
    if (newVia != null) {
      int fromLayer;
      int toLayer;
      if (oldLayer < newLayer) {
        fromLayer = oldLayer + 1;
        toLayer = newLayer;
      } else {
        fromLayer = newLayer;
        toLayer = oldLayer - 1;
      }
      for (Item currentItem : newVia.getNormalContacts()) {
        if (currentItem instanceof ConductionArea currentArea) {
          if (currentArea.getLayer() >= fromLayer && currentArea.getLayer() <= toLayer) {
            connectedToPlane = true;
            break;
          }
        }
      }
    }
    if (connectedToPlane) {
      return "plane";
    }
    ws.layer = newLayer;
    board.generateSnapshot();
    return "changed";
  }

  static void cancel(RoutingBoard board, Route route, int[] nets, Ws ws) {
    Trace tail = board.getTraceTail(route.getLastCorner(), ws.layer, nets);
    if (tail != null) {
      Collection<Item> removeItems = tail.getConnectionItems(Item.StopConnectionOption.VIA);
      if (ws.push) {
        board.removeItemsAndPullTight(removeItems, ws.tidy, ws.accuracy);
      } else {
        board.removeItems(removeItems);
      }
    }
  }

  // ---- the canonical item dump (tests/support/item_dump.rs) ----

  static String fpoint(FloatPoint f) {
    return "f" + Boards.bits(f.x) + "," + Boards.bits(f.y);
  }

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
