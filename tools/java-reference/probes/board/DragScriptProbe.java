import app.freerouting.Freerouting;
import app.freerouting.board.actions.ItemSelectionFilter;
import app.freerouting.board.actions.MoveComponent;
import app.freerouting.board.facade.RoutingBoard;
import app.freerouting.board.model.items.DrillItem;
import app.freerouting.board.model.items.Item;
import app.freerouting.board.model.items.Pin;
import app.freerouting.board.model.items.Trace;
import app.freerouting.board.model.items.Via;
import app.freerouting.board.model.structure.AngleRestriction;
import app.freerouting.board.model.structure.Component;
import app.freerouting.board.model.structure.FixedState;
import app.freerouting.board.trace.PolylineTrace;
import app.freerouting.core.RoutingJob;
import app.freerouting.geometry.planar.FloatPoint;
import app.freerouting.geometry.planar.IntPoint;
import app.freerouting.geometry.planar.IntVector;
import app.freerouting.geometry.planar.Point;
import app.freerouting.geometry.planar.Polyline;
import app.freerouting.geometry.planar.Vector;
import app.freerouting.gui.interactive.Route;
import app.freerouting.io.specctra.SesReader;
import app.freerouting.management.BoardLoader;
import app.freerouting.rules.Nets;
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
 * Board probe {@code drag-script} (I02.4): dragging drill items and components with {@code
 * board.actions.MoveComponent} and making space with {@code gui.interactive.Route} on the hidden
 * net, headless, mirrored by {@code crates/freerouting/tests/interactive_drag.rs}.
 *
 * <p>The drag states need a {@code GuiBoardManager} (Swing), so this probe transcribes their board
 * calls: {@code DragState.getInstance} with {@code findItemToMove} and {@code selectItemToMove},
 * {@code DragItemState.moveTo} (the angle projection, the user-fixed check, the two tries of {@code
 * new MoveComponent(item, delta, 99, 5)} with the shortened second move, the snapshot on the first
 * successful step, {@code insert(tidy, accuracy)}) with {@code DragState.mouseDragged}'s undo after a
 * failed insert, {@code DragItemState.buttonReleased}'s changed nets, and {@code MakeSpaceState}'s
 * constructor, {@code moveTo} and {@code buttonReleased}. The settings are fixed: current layer 0
 * (the picked trace's layer for make-space), select on all visible layers, all layers visible,
 * component dragging on, the default item selection filter, tidy width {@code Integer.MAX_VALUE},
 * accuracy 500, obstacle highlight off. {@code MoveComponent}'s and {@code Route}'s work limits are
 * their own.
 *
 * <p>Boards: the five of {@link #BOARDS}: stage {@code ses} with the fixtures.tsv session, or
 * {@code load} as loaded (Issue957, whose DSN holds wiring); every other row exits 3. The session
 * import (and the DSN) fixes the wiring, so the probe first unfixes every user-fixed trace and via
 * ({@link #unfixWiring}); a user-fixed item neither drags nor shoves. One {@code new
 * Random(SEED)} draws, in this order: {@value #VIA_DRAGS} vias (highest id first, drawn from the
 * current list before each drag), each dragged from its centre in {@value #DRAG_STEPS} pointer
 * steps of {@value #STEP} units in one of the 8 45-degree directions; {@value #PIN_DRAGS} pins
 * with a net (highest id first), dragged the same way (a pin of a component drags the component);
 * {@value #SPACE_DRAGS} make-space drags: a trace (highest id first), the middle of its first
 * segment, a direction, a press at {@code middle + 4 h dir} on the trace's layer ({@code h} the
 * board's smallest trace half width) and {@value #SPACE_STEPS} pointer steps of {@code h} towards
 * {@code middle - 4 h dir}. A press that starts something else is recorded and not dragged.
 *
 * <p>Lines: each press, step and release starts with {@code "stage"} (so a digest has one section
 * per line); a step gives the outcome, the move vector, the dragged item's centre, its component's
 * location and {@code maxGeneratedId}; a make-space step gives {@code nextCorner}'s result, the
 * route end, the shove-failing obstacle and whether the pointer is moved. The canonical item dump
 * follows every 10th step and the end of each drag phase.
 */
public class DragScriptProbe {
  static final long SEED = 20261016L;
  static final int VIA_DRAGS = 10;
  static final int PIN_DRAGS = 5;
  static final int SPACE_DRAGS = 3;
  static final int DRAG_STEPS = 5;
  static final int SPACE_STEPS = 8;
  static final double STEP = 200.0;
  static final int[][] DIRS45 = {{1, 0}, {1, 1}, {0, 1}, {-1, 1}, {-1, 0}, {-1, -1}, {0, -1}, {1, -1}};
  static final Map<String, Boolean> BOARDS =
      Map.of(
          "Issue026-J2_reference", true,
          "Issue508-DAC2020_bm05", true,
          "Issue508-DAC2020_bm08", true,
          "Issue143-rpi_splitter", true,
          "Issue957-smoke-ldo-led", false);

  public static void main(String[] args) throws Exception {
    String board = Boards.board(args[0]);
    Boolean withSession = BOARDS.get(board);
    if (withSession == null) {
      System.exit(3);
    }
    Boards.Out out = new Boards.Out("drag-script", args[0]);
    try {
      long started = System.nanoTime();
      RoutingBoard routingBoard = Boards.load(args[0]);
      String stage = "load";
      if (withSession) {
        Boards.importSession(routingBoard, args[1]);
        stage = "ses";
      }
      unfixWiring(routingBoard);
      new DragScriptProbe(out, routingBoard, stage).script();
      System.err.printf("drag-script: %.1f ms%n", (System.nanoTime() - started) / 1e6);
    } catch (Throwable t) {
      out.exception(t);
    }
    out.write(args[2]);
  }

  /**
   * Unfixes the user-fixed traces and vias (highest id first): Java's session import and the DSN
   * wiring fix them, and a user-fixed item neither drags nor shoves.
   */
  static void unfixWiring(RoutingBoard board) {
    List<Item> items = new ArrayList<>(board.getItems());
    items.sort((a, b) -> Integer.compare(b.getId(), a.getId()));
    for (Item item : items) {
      if ((item instanceof Trace || item instanceof Via) && item.getFixedState() == FixedState.USER_FIXED) {
        item.setFixedState(FixedState.UNFIXED);
      }
    }
  }

  final Boards.Out out;
  final RoutingBoard board;
  final String stage;
  int layer = 0;
  int steps = 0;

  DragScriptProbe(Boards.Out out, RoutingBoard board, String stage) {
    this.out = out;
    this.board = board;
    this.stage = stage;
  }

  Boards.Line line(String phase, int n) {
    return new Boards.Line().str("stage", stage).str("phase", phase).num("drag", n);
  }

  long maxId() {
    return board.communication.idGenerator.maxGeneratedId();
  }

  List<Item> sortedItems() {
    List<Item> items = new ArrayList<>(board.getItems());
    items.sort((a, b) -> Integer.compare(b.getId(), a.getId()));
    return items;
  }

  void script() throws Exception {
    Random random = new Random(SEED);
    for (int n = 0; n < VIA_DRAGS; n++) {
      List<Item> vias = new ArrayList<>();
      for (Item item : sortedItems()) {
        if (item instanceof Via) {
          vias.add(item);
        }
      }
      if (vias.isEmpty()) {
        out.line(line("via", n).str("skip", "no vias").end());
        continue;
      }
      DrillItem via = (DrillItem) vias.get(random.nextInt(vias.size()));
      int[] dir = DIRS45[random.nextInt(8)];
      itemDrag("via", n, via.getCenter().toFloat(), dir);
    }
    dump();
    for (int n = 0; n < PIN_DRAGS; n++) {
      List<Item> pins = new ArrayList<>();
      for (Item item : sortedItems()) {
        if (item instanceof Pin && item.netCount() > 0) {
          pins.add(item);
        }
      }
      if (pins.isEmpty()) {
        out.line(line("pin", n).str("skip", "no pins").end());
        continue;
      }
      DrillItem pin = (DrillItem) pins.get(random.nextInt(pins.size()));
      int[] dir = DIRS45[random.nextInt(8)];
      itemDrag("pin", n, pin.getCenter().toFloat(), dir);
    }
    dump();
    for (int n = 0; n < SPACE_DRAGS; n++) {
      List<PolylineTrace> traces = new ArrayList<>();
      for (Item item : sortedItems()) {
        if (item instanceof PolylineTrace trace) {
          traces.add(trace);
        }
      }
      if (traces.isEmpty()) {
        out.line(line("space", n).str("skip", "no traces").end());
        continue;
      }
      PolylineTrace trace = traces.get(random.nextInt(traces.size()));
      int[] dir = DIRS45[random.nextInt(8)];
      Polyline polyline = trace.polyline();
      FloatPoint middle = polyline.cornerApprox(0).middlePoint(polyline.cornerApprox(1));
      makeSpace(n, trace.getLayer(), middle, dir);
    }
    dump();
  }

  // ---- DragState.findItemToMove and selectItemToMove, transcribed ----

  /** The item a press at {@code location} drags; {@code found[0]} tells if anything was found. */
  Item findItemToMove(FloatPoint location, boolean[] found) {
    int tryCount = 1 + board.getLayerCount();
    int currentLayer = layer;
    int pickLayer = currentLayer;
    boolean itemFound = false;
    for (int i = 0; i < tryCount; i++) {
      if (i == 0 || pickLayer != currentLayer) {
        Collection<Item> foundItems =
            board.pickItems(location.round(), pickLayer, new ItemSelectionFilter());
        Item itemToMove = null;
        for (Item currentItem : foundItems) {
          itemFound = true;
          if (currentItem instanceof Trace) {
            continue;
          }
          itemToMove = currentItem;
          if (currentItem instanceof DrillItem) {
            break;
          }
        }
        if (itemToMove != null) {
          found[0] = true;
          return itemToMove;
        }
      }
      pickLayer = i;
    }
    found[0] = itemFound;
    return null;
  }

  String pressKind(Item item, boolean found) {
    return item != null ? "item" : found ? "none" : "makespace";
  }

  // ---- DragItemState, transcribed ----

  void itemDrag(String phase, int n, FloatPoint at, int[] dir) throws Exception {
    layer = 0;
    boolean[] found = new boolean[1];
    Item item = findItemToMove(at, found);
    String kind = pressKind(item, found[0]);
    Boards.Line press = line(phase, n).str("kind", "press").str("at", fpoint(at)).str("press", kind);
    if (item == null) {
      out.line(press.num("maxGeneratedId", maxId()).end());
      return;
    }
    int group = item.getComponentId() > 0 ? board.getComponentItems(item.getComponentId()).size() : 1;
    out.line(press.num("item", item.getId()).num("group", group).num("maxGeneratedId", maxId()).end());
    FloatPoint previous = at;
    boolean dragged = false;
    for (int k = 1; k <= DRAG_STEPS; k++) {
      FloatPoint to = new FloatPoint(at.x + dir[0] * STEP * k, at.y + dir[1] * STEP * k);
      Boards.Line step = line(phase, n).str("kind", "step").num("k", k).str("to", fpoint(to));
      IntPoint roundedLocation = to.round();
      IntPoint fromLocation = previous.round();
      if (board.rules.getTraceAngleRestriction() == AngleRestriction.NINETY_DEGREE) {
        roundedLocation = roundedLocation.orthogonalProjection(fromLocation);
      } else if (board.rules.getTraceAngleRestriction() == AngleRestriction.FORTYFIVE_DEGREE) {
        roundedLocation = roundedLocation.fortyfiveDegreeProjection(fromLocation);
      }
      String outcome;
      String vector = "-";
      String changed = "-";
      boolean failed = false;
      if (roundedLocation.equals(fromLocation)) {
        outcome = "unchanged";
      } else if (item.isUserFixed()) {
        outcome = "fixed";
      } else {
        MoveComponent moveComponent = null;
        Vector relCoor = roundedLocation.differenceBy(fromLocation);
        double length = relCoor.lengthApprox();
        boolean shoveOk = false;
        for (int i = 0; i < 2; i++) {
          moveComponent = new MoveComponent(item, relCoor, 99, 5);
          if (moveComponent.check()) {
            shoveOk = true;
            break;
          }
          if (i == 0) {
            double sampleWidth = 2 * board.getMinTraceHalfWidth();
            double minDrillWidth = moveComponent.getMinDrillItemWidthWithTraces();
            if (minDrillWidth > 0 && sampleWidth >= minDrillWidth) {
              sampleWidth = Math.max(1.0, minDrillWidth - 1.0);
            }
            if (length > sampleWidth) {
              relCoor = relCoor.changeLengthApprox(sampleWidth);
            }
          }
        }
        if (!shoveOk) {
          outcome = "blocked";
        } else {
          if (!dragged) {
            board.generateSnapshot();
            dragged = true;
          }
          vector = vector(relCoor);
          if (!moveComponent.insert(Integer.MAX_VALUE, 500)) {
            Set<Integer> changedNets = new TreeSet<>();
            board.undo(changedNets);
            outcome = "failed";
            changed = joinInts(changedNets);
            failed = true;
          } else {
            outcome = "moved";
            previous = previous.add(relCoor.toFloat());
          }
        }
      }
      step.str("outcome", outcome).str("vector", vector).str("changed", changed);
      out.line(itemState(step, item).str("previous", fpoint(previous)).num("maxGeneratedId", maxId()).end());
      countStep();
      if (failed) {
        return;
      }
    }
    // DragItemState.buttonReleased
    Set<Integer> changedNets = new TreeSet<>();
    if (dragged) {
      if (item.getComponentId() == 0) {
        for (int i = 0; i < item.netCount(); i++) {
          changedNets.add(item.getNetNumber(i));
        }
      } else {
        for (Item moved : board.getComponentItems(item.getComponentId())) {
          for (int i = 0; i < moved.netCount(); i++) {
            changedNets.add(moved.getNetNumber(i));
          }
        }
      }
    }
    out.line(line(phase, n).str("kind", "release").str("changed", joinInts(changedNets)).num("maxGeneratedId", maxId()).end());
  }

  /**
   * The centre of the dragged item and its component's location. The item is looked up by id: after
   * an undo the board holds the restored copy, and the probe's reference is stale.
   */
  Boards.Line itemState(Boards.Line line, Item dragged) {
    Item item = dragged;
    for (Item current : board.getItems()) {
      if (current.getId() == dragged.getId()) {
        item = current;
        break;
      }
    }
    String center = item instanceof DrillItem drill ? point(drill.getCenter()) : "-";
    String location = "-";
    if (item.getComponentId() > 0) {
      Component component = board.components.get(item.getComponentId());
      if (component != null && component.getLocation() != null) {
        location = point(component.getLocation());
      }
    }
    return line.str("center", center).str("location", location);
  }

  // ---- MakeSpaceState, transcribed ----

  void makeSpace(int n, int traceLayer, FloatPoint middle, int[] dir) throws Exception {
    layer = traceLayer;
    double h = board.getMinTraceHalfWidth();
    FloatPoint at = new FloatPoint(middle.x + dir[0] * 4 * h, middle.y + dir[1] * 4 * h);
    boolean[] found = new boolean[1];
    Item item = findItemToMove(at, found);
    String kind = pressKind(item, found[0]);
    Boards.Line press = line("space", n).str("kind", "press").num("layer", layer).str("at", fpoint(at)).str("press", kind);
    if (!kind.equals("makespace")) {
      out.line(press.num("maxGeneratedId", maxId()).end());
      return;
    }
    out.line(press.num("maxGeneratedId", maxId()).end());
    int[] shoveTraceWidthArr = new int[board.getLayerCount()];
    boolean[] layerActiveArr = new boolean[shoveTraceWidthArr.length];
    int shoveTraceWidth = Math.min(100, board.getMinTraceHalfWidth() / 10);
    shoveTraceWidth = Math.max(shoveTraceWidth, 5);
    for (int i = 0; i < shoveTraceWidthArr.length; i++) {
      shoveTraceWidthArr[i] = shoveTraceWidth;
      layerActiveArr[i] = true;
    }
    Route route =
        new Route(at.round(), layer, shoveTraceWidthArr, layerActiveArr, new int[] {Nets.hidden_net_number}, 0,
            ViaRule.EMPTY, true, Integer.MAX_VALUE, 500, null, null, board, false, false, false, false);
    boolean dragged = false;
    for (int k = 1; k <= SPACE_STEPS; k++) {
      FloatPoint to = new FloatPoint(middle.x + dir[0] * (4 - k) * h, middle.y + dir[1] * (4 - k) * h);
      if (!dragged) {
        board.generateSnapshot();
        dragged = true;
      }
      boolean ended = route.nextCorner(to);
      Point routeEnd = route.getLastCorner();
      boolean warp =
          board.rules.getTraceAngleRestriction() == AngleRestriction.NONE && !routeEnd.equals(to.round());
      out.line(
          line("space", n)
              .str("kind", "step")
              .num("k", k)
              .str("to", fpoint(to))
              .bool("ended", ended)
              .str("lastCorner", point(routeEnd))
              .num("failing", id(field(route, "shoveFailingObstacle")))
              .bool("warp", warp)
              .num("maxGeneratedId", maxId())
              .end());
      countStep();
    }
    Collection<Item> hidden = board.getConnectableItems(Nets.hidden_net_number);
    int removed = hidden.size();
    board.removeItems(hidden);
    out.line(line("space", n).str("kind", "release").num("removed", removed).num("maxGeneratedId", maxId()).end());
  }

  static Object field(Route route, String name) throws Exception {
    Field f = Route.class.getDeclaredField(name);
    f.setAccessible(true);
    return f.get(route);
  }

  static long id(Object item) {
    return item == null ? -1 : ((Item) item).getId();
  }

  void countStep() {
    steps++;
    if (steps % 10 == 0) {
      dump();
    }
  }

  static String joinInts(Collection<Integer> values) {
    StringBuilder text = new StringBuilder();
    for (Integer value : values) {
      text.append(text.length() > 0 ? "," : "").append(value);
    }
    return text.toString();
  }

  static String vector(Vector v) {
    if (v instanceof IntVector iv) {
      return iv.x + "," + iv.y;
    }
    FloatPoint f = v.toFloat();
    return "f" + Boards.bits(f.x) + "," + Boards.bits(f.y);
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
  void dump() {
    for (Item item : sortedItems()) {
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
            .num("maxGeneratedId", maxId())
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
