package app.freerouting.autoroute.maze;

import app.freerouting.Freerouting;
import app.freerouting.autoroute.drill.DrillPage;
import app.freerouting.autoroute.drill.ExpansionDrill;
import app.freerouting.autoroute.expansion.IncompleteFreeSpaceExpansionRoom;
import app.freerouting.autoroute.expansion.ObstacleExpansionRoom;
import app.freerouting.autoroute.maze.AutorouteControl.ExpansionCostFactor;
import app.freerouting.autoroute.maze.AutorouteControl.ViaCost;
import app.freerouting.autoroute.maze.AutorouteControl.ViaMask;
import app.freerouting.board.actions.ItemIdGenerator;
import app.freerouting.board.facade.RoutingBoard;
import app.freerouting.board.model.items.ConductionArea;
import app.freerouting.board.model.items.Item;
import app.freerouting.board.searchtree.ShapeSearchTree;
import app.freerouting.core.RoutingJob;
import app.freerouting.geometry.planar.FloatPoint;
import app.freerouting.geometry.planar.IntBox;
import app.freerouting.geometry.planar.TileShape;
import app.freerouting.io.specctra.RulesReader;
import app.freerouting.management.HeadlessBoardManager;
import app.freerouting.rules.Net;
import app.freerouting.settings.GlobalSettings;
import app.freerouting.settings.RouterSettings;
import app.freerouting.settings.SettingsMerger;
import app.freerouting.settings.sources.ApiSettings;
import app.freerouting.settings.sources.DefaultSettings;
import app.freerouting.settings.sources.DsnFileSettings;
import app.freerouting.settings.sources.RulesFileSettings;
import java.io.ByteArrayInputStream;
import java.io.File;
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
 * The autoroute probe (A00 "Probes", A01): the Java side of the Rust tests
 * crates/freerouting/tests/autoroute_probe_*.rs. One file for every A checklist; each appends a
 * kind and none rewrites another's. In package app.freerouting.autoroute.maze so package-private
 * maze state is read directly; private state elsewhere is read by reflection.
 *
 * <pre>
 * probe.sh PIN AutorouteProbe.java KIND FIXTURE.dsn|- OUT.jsonl
 * </pre>
 *
 * Kinds (A01):
 *
 * <ul>
 *   <li>selftest (fixture -): fixed values through the writer, so both writers are checked.
 *   <li>connections: the sampled connections, per variant (ripup off, then on): net, item, plane
 *       flag, state, start and destination ids.
 *   <li>java-ids: the getId formulas on 1,000 inputs each from new Random(7) (nextInt() over the
 *       whole int range), then the ids of real objects built on the first 200 tree shapes.
 *   <li>control: every field of AutorouteControl for every net, built as the batch router builds
 *       it, then after a second rebuildViaInfo.
 *   <li>distance: per sampled connection a DestinationDistance joined as MazeSearchEngine.init
 *       joins it, evaluated at 1,000 points and 1,000 boxes from new Random(11) on every layer.
 *   <li>start-rooms (A02.7): per sampled connection and variant, after
 *       MazeSearchEngine.getInstance: the complete rooms in list order with their doors and target
 *       doors, the incomplete rooms left, the maze queue in order, the room counter, and the
 *       autoroute tree's walk hash after AutorouteEngine.clear().
 *   <li>door-sections (A02.5): ExpansionDoor.getSectionSegments of a hand-built pair of doors at
 *       four offsets, then of the first 100 doors of the routable sampled connections.
 *   <li>drills (A03.5): per sampled connection and variant, after MazeSearchEngine.getInstance:
 *       DrillPage.getDrills (attachSmd = ctrl.attachSmdAllowed) on every page of the engine's
 *       page array in row-major order, each page with its drills (location, shape, layers, the
 *       room id per layer), then the room counter and the tree's walk hash after clear(). A
 *       third variant "smd" repeats ripup off with attachSmd forced on.
 *   <li>maze-log (A04.8): per sampled connection and variant, MazeSearchEngine.getInstance, then
 *       occupyNextElement step by step (at most 200,000 calls). Before each call the queue is
 *       walked from its first element: the element the call expands (the first whose section
 *       is unoccupied) is printed with its door kind and id, section, key bits, next room,
 *       backtrack door and flags, then the occupied ones popped before it, whether its
 *       section is occupied after the call, and the thread's WorkMeter units since the
 *       engine's connection was set. At the end: the destination, the room counter, the queue
 *       size and the units; then the tree's walk hash after clear(). No WorkLimit is passed.
 *   <li>maze-routed (A04.8): maze-log through pre-routed traces. The board's traces and vias
 *       are unfixed; then, for up to 12 nets with route items, the net's route items are
 *       removed and its sampled connection is searched as in maze-log (see mazeRouted).
 * </ul>
 *
 * Output: one JSON object per line (keys in a fixed order, integers in decimal, doubles as "0x" +
 * the 16 hex digits of their raw bits); the first line is the header. A Java exception is a
 * record {"exception":...,"at":...} and ends the output.
 */
public class AutorouteProbe {

  /** At most this many connections per board (the sampler, A01 decisions, step 6). */
  static final int MAX_CONNECTIONS = 200;

  static final int ID_SAMPLES = 1000;
  static final int TREE_SHAPES = 200;
  static final int DISTANCE_SAMPLES = 1000;

  public static void main(String[] args) throws Exception {
    String kind = args[0];
    String dsn = args[1];
    Out out = new Out(kind, dsn.equals("-") ? kind : board(dsn));
    try {
      switch (kind) {
        case "selftest" -> selftest(out);
        case "connections" -> connections(out, Loaded.load(dsn));
        case "java-ids" -> javaIds(out, Loaded.load(dsn));
        case "control" -> control(out, Loaded.load(dsn));
        case "distance" -> distance(out, Loaded.load(dsn));
        case "start-rooms" -> startRooms(out, Loaded.load(dsn));
        case "door-sections" -> doorSections(out, Loaded.load(dsn));
        case "drills" -> drills(out, Loaded.load(dsn));
        case "maze-log" -> mazeLog(out, Loaded.load(dsn));
        case "maze-routed" -> mazeRouted(out, Loaded.load(dsn));
        default -> throw new IllegalArgumentException("unknown kind " + kind);
      }
    } catch (Throwable t) {
      out.exception(t);
    }
    out.write(args[2]);
  }

  // ------------------------------------------------------------------------------------------
  // selftest
  // ------------------------------------------------------------------------------------------

  static void selftest(Out out) {
    out.line(new Line().str("stage", "ints").num("zero", 0).num("intMin", Integer.MIN_VALUE)
        .num("intMax", Integer.MAX_VALUE).num("longMin", Long.MIN_VALUE).end());
    out.line(new Line().str("stage", "doubles").dbl("negZero", -0.0).dbl("tenth", 0.1)
        .dbl("minValue", Double.MIN_VALUE).dbl("minNormal", Double.MIN_NORMAL)
        .dbl("max", Double.MAX_VALUE).dbl("nan", Double.NaN)
        .dbl("negInf", Double.NEGATIVE_INFINITY).dbl("intMaxAsDouble", Integer.MAX_VALUE).end());
    out.line(new Line().str("stage", "other").bool("yes", true).bool("no", false)
        .str("text", "quote\" backslash\\ tab\t ctl\u0001 é").end());
    out.line(new Line().str("stage", "lists").raw("empty", "[]")
        .raw("nested", "[" + ints(List.of(1, -2)) + ",[]," + "[" + quoted(bits(0.5)) + ",true]]")
        .end());
    out.line(new Line().str("stage", "fnv")
        .str("ab", hex(fnvLines(List.of("a", "b")))).str("none", hex(fnvLines(List.of()))).end());
  }

  // ------------------------------------------------------------------------------------------
  // The connection sampler (A01 decisions)
  // ------------------------------------------------------------------------------------------

  /** One sampled connection: the net, the item and the route sets of AutorouteConnectionRouter. */
  static final class Conn {
    int net;
    Item item;
    boolean plane;
    String state = "";
    Set<Item> start;
    Set<Item> dest;
  }

  static List<Conn> sample(RoutingBoard board) {
    List<Conn> result = new ArrayList<>();
    int maxNet = board.rules.nets.maxNetNumber();
    for (int net = 1; net <= maxNet && result.size() < MAX_CONNECTIONS; net++) {
      Item chosen = null;
      for (Item item : board.getConnectableItems(net)) {
        if (!item.hasIgnoredNets() && !item.getUnconnectedSet(net).isEmpty()) {
          chosen = item;
          break;
        }
      }
      if (chosen == null) {
        continue;
      }
      Conn conn = new Conn();
      conn.net = net;
      conn.item = chosen;
      // AutorouteConnectionRouter.route, the start and destination sets.
      Net routeNet = board.rules.nets.get(net);
      conn.plane = routeNet != null && routeNet.containsPlane();
      Set<Item> unconnected = chosen.getUnconnectedSet(net);
      Set<Item> connected = chosen.getConnectedSet(net);
      if (conn.plane) {
        for (Item item : connected) {
          if (item instanceof ConductionArea) {
            conn.state = "CONNECTED_TO_PLANE";
            break;
          }
        }
        conn.start = connected;
        Set<Item> planeTargets = board.powerPlaneAreas(unconnected);
        conn.dest = planeTargets.isEmpty() ? unconnected : planeTargets;
      } else {
        conn.start = unconnected;
        conn.dest = connected;
      }
      if (!conn.state.isEmpty()) {
        conn.start = Set.of();
        conn.dest = Set.of();
      }
      result.add(conn);
    }
    return result;
  }

  static void sampleLine(Out out, RoutingBoard board, List<Conn> conns) {
    out.line(new Line().str("stage", "sample").num("maxConnections", MAX_CONNECTIONS)
        .num("maxNetNumber", board.rules.nets.maxNetNumber()).num("connections", conns.size())
        .end());
  }

  /** The control of a connection as AutorouteConnectionRouter builds it, ripup on or off. */
  static AutorouteControl routerControl(Loaded l, int net, boolean ripup) {
    RouterSettings s = l.job.routerSettings;
    Net routeNet = l.board.rules.nets.get(net);
    boolean plane = routeNet != null && routeNet.containsPlane();
    int viaCosts = plane ? s.getPlaneViaCosts() : s.getViaCosts();
    AutorouteControl c = new AutorouteControl(l.board, net, s, viaCosts, s.getTraceCosts());
    if (ripup) {
      c.ripupAllowed = true;
      c.ripupCosts = s.getStartRipupCosts() * 1;
    }
    c.removeUnconnectedVias = !s.isFanoutEnabled();
    return c;
  }

  static void connections(Out out, Loaded l) {
    List<Conn> conns = sample(l.board);
    sampleLine(out, l.board, conns);
    // The ripup values of each variant, without building the control (which throws on boards
    // without a via rule): the constructor's defaults with ripup off, pass 1's with ripup on.
    int startRipupCosts = l.job.routerSettings.getStartRipupCosts();
    for (String variant : new String[] {"off", "on"}) {
      boolean ripup = variant.equals("on");
      for (Conn conn : conns) {
        out.line(new Line().str("stage", variant + ":" + conn.net).num("net", conn.net)
            .num("item", conn.item.getId()).bool("plane", conn.plane).str("state", conn.state)
            .bool("ripupAllowed", ripup).num("ripupCosts", ripup ? startRipupCosts : 1000)
            .raw("start", ids(conn.start)).raw("dest", ids(conn.dest)).end());
      }
    }
  }

  // ------------------------------------------------------------------------------------------
  // java-ids (A01.2)
  // ------------------------------------------------------------------------------------------

  /** The getId expressions, verbatim from the classes at the pin (int arithmetic). */
  static int doorId(int id1, int id2) {
    return Math.min(id1, id2) * 31 + Math.max(id1, id2); // ExpansionDoor.java:189
  }

  static int targetDoorId(int itemId, int roomId) {
    return 31 * itemId + roomId; // TargetItemExpansionDoor.java:73
  }

  static int obstacleRoomId(int itemId, int index) {
    return (itemId << 10) | index; // ObstacleExpansionRoom.java:50
  }

  static int incompleteRoomId(int shapeId, int layer) {
    return 31 * shapeId + layer; // IncompleteFreeSpaceExpansionRoom.java:40
  }

  static int drillId(int locationId, int first, int last) {
    return 31 * (31 * locationId + first) + last; // ExpansionDrill.java:129
  }

  static int drillPageId(int shapeId, int net) {
    return 31 * shapeId + net; // DrillPage.java:195
  }

  static void javaIds(Out out, Loaded l) {
    Random random = new Random(7);
    String[] formulas = {"door", "target", "obstacle", "incomplete", "drill", "page"};
    for (String formula : formulas) {
      out.line(new Line().str("stage", formula).num("samples", ID_SAMPLES).end());
      for (int i = 0; i < ID_SAMPLES; i++) {
        int a = random.nextInt();
        int b = random.nextInt();
        int c = formula.equals("drill") ? random.nextInt() : 0;
        int id =
            switch (formula) {
              case "door" -> doorId(a, b);
              case "target" -> targetDoorId(a, b);
              case "obstacle" -> obstacleRoomId(a, b);
              case "incomplete" -> incompleteRoomId(a, b);
              case "drill" -> drillId(a, b, c);
              default -> drillPageId(a, b);
            };
        out.line(new Line().num("a", a).num("b", b).num("c", c).num("id", id).end());
      }
    }
    // Real objects on the first tree shapes of the default tree, in item-list order.
    RoutingBoard board = l.board;
    ShapeSearchTree tree = board.searchTreeManager.getDefaultTree();
    out.line(new Line().str("stage", "tree-shapes").num("max", TREE_SHAPES).end());
    int count = 0;
    for (Item item : board.getItems()) {
      for (int i = 0; i < item.treeShapeCount(tree) && count < TREE_SHAPES; i++) {
        TileShape shape = item.getTreeShape(tree, i);
        if (shape == null) {
          continue;
        }
        int layer = item.shapeLayer(i);
        ObstacleExpansionRoom obstacle = new ObstacleExpansionRoom(item, i, tree);
        IncompleteFreeSpaceExpansionRoom incomplete =
            new IncompleteFreeSpaceExpansionRoom(shape, layer, null);
        IntBox box = shape.boundingBox();
        ExpansionDrill drill = new ExpansionDrill(shape, box.ll, 0, board.getLayerCount() - 1);
        DrillPage page = new DrillPage(box, board);
        out.line(new Line().num("item", item.getId()).num("index", i).num("layer", layer)
            .num("shape", shape.getId()).num("box", box.getId()).num("obstacle", obstacle.getId())
            .num("incomplete", incomplete.getId()).num("drill", drill.getId())
            .num("page", page.getId())
            .num("door", doorId(obstacle.getId(), incomplete.getId()))
            .num("target", targetDoorId(item.getId(), incomplete.getId())).end());
        count++;
      }
      if (count >= TREE_SHAPES) {
        break;
      }
    }
  }

  // ------------------------------------------------------------------------------------------
  // control (A01.5)
  // ------------------------------------------------------------------------------------------

  static void control(Out out, Loaded l) {
    RoutingBoard board = l.board;
    RouterSettings s = l.job.routerSettings;
    int maxNet = board.rules.nets.maxNetNumber();
    out.line(new Line().str("stage", "board").num("maxNetNumber", maxNet)
        .num("layerCount", board.getLayerCount()).num("viaCosts", s.getViaCosts())
        .num("planeViaCosts", s.getPlaneViaCosts()).num("startRipupCosts", s.getStartRipupCosts())
        .bool("fanout", s.isFanoutEnabled()).end());
    for (int net = 1; net <= maxNet; net++) {
      AutorouteControl c;
      try {
        c = routerControl(l, net, true);
      } catch (RuntimeException e) {
        out.line(new Line().str("stage", "net:" + net)
            .str("exception", e.getClass().getSimpleName()).end());
        continue;
      }
      out.line(controlLine("net:" + net, c));
      Net routeNet = board.rules.nets.get(net);
      boolean plane = routeNet != null && routeNet.containsPlane();
      c.rebuildViaInfo(board, plane ? s.getPlaneViaCosts() : s.getViaCosts(), net);
      out.line(new Line().str("stage", "rebuilt:" + net).raw("viaRadii", bitsList(c.viaRadii))
          .dbl("maxViaRadius", c.maxViaRadius).dbl("minNormalViaCost", c.minNormalViaCost)
          .dbl("minCheapViaCost", c.minCheapViaCost).end());
    }
  }

  static String controlLine(String stage, AutorouteControl c) {
    StringBuilder costs = new StringBuilder("[");
    for (ExpansionCostFactor f : c.traceCosts) {
      if (costs.length() > 1) {
        costs.append(',');
      }
      costs.append('[').append(quoted(bits(f.horizontal()))).append(',')
          .append(quoted(bits(f.vertical()))).append(']');
    }
    costs.append(']');
    StringBuilder add = new StringBuilder("[");
    for (ViaCost v : c.addViaCosts) {
      if (add.length() > 1) {
        add.append(',');
      }
      add.append(intArray(v.toLayer));
    }
    add.append(']');
    StringBuilder masks = new StringBuilder("[");
    for (ViaMask m : c.viaInfos) {
      if (masks.length() > 1) {
        masks.append(',');
      }
      masks.append('[').append(m.fromLayer).append(',').append(m.toLayer).append(',')
          .append(m.attachSmdAllowed).append(']');
    }
    masks.append(']');
    String center =
        c.fanoutStartPinCenter == null
            ? "[]"
            : "[" + quoted(c.fanoutStartPinCenter.toString()) + "]";
    return new Line().str("stage", stage).num("netNumber", c.netNumber)
        .num("layerCount", c.layerCount).raw("traceCosts", costs.toString())
        .raw("bendCosts", bitsList(c.bendCosts)).bool("withNeckdown", c.withNeckdown)
        .raw("layerActive", boolArray(c.layerActive)).raw("traceHalfWidth", intArray(c.traceHalfWidth))
        .raw("compensatedTraceHalfWidth", intArray(c.compensatedTraceHalfWidth))
        .raw("viaRadii", bitsList(c.viaRadii)).raw("addViaCosts", add.toString())
        .num("traceClearanceClassIndex", c.traceClearanceClassIndex)
        .bool("viasAllowed", c.viasAllowed).bool("attachSmdAllowed", c.attachSmdAllowed)
        .dbl("minNormalViaCost", c.minNormalViaCost).bool("ripupAllowed", c.ripupAllowed)
        .num("ripupCosts", c.ripupCosts).num("ripupPassNo", c.ripupPassNo)
        .bool("isFanout", c.isFanout).bool("isPlaneFanout", c.isPlaneFanout)
        .raw("fanoutStartPinCenter", center).num("fanoutStartPinLayer", c.fanoutStartPinLayer)
        .bool("removeUnconnectedVias", c.removeUnconnectedVias).str("viaRule", c.viaRule.name)
        .num("viaClearanceClass", c.viaClearanceClass).raw("viaInfos", masks.toString())
        .num("viaLowerBound", c.viaLowerBound).num("viaUpperBound", c.viaUpperBound)
        .dbl("maxViaRadius", c.maxViaRadius).num("tidyRegionWidth", c.tidyRegionWidth)
        .num("pullTightAccuracy", c.pullTightAccuracy)
        .num("maxShoveTraceRecursionDepth", c.maxShoveTraceRecursionDepth)
        .num("maxShoveViaRecursionDepth", c.maxShoveViaRecursionDepth)
        .num("maxSpringOverRecursionDepth", c.maxSpringOverRecursionDepth)
        .dbl("minCheapViaCost", c.minCheapViaCost).end();
  }

  // ------------------------------------------------------------------------------------------
  // distance (A01.5)
  // ------------------------------------------------------------------------------------------

  static void distance(Out out, Loaded l) {
    RoutingBoard board = l.board;
    List<Conn> conns = sample(board);
    sampleLine(out, board, conns);
    IntBox bb = board.boundingBox;
    int layers = board.getLayerCount();
    for (Conn conn : conns) {
      if (!conn.state.isEmpty()) {
        out.line(new Line().str("stage", "conn:" + conn.net).num("net", conn.net)
            .str("state", conn.state).end());
        continue;
      }
      AutorouteControl c;
      try {
        c = routerControl(l, conn.net, false);
      } catch (RuntimeException e) {
        out.line(new Line().str("stage", "conn:" + conn.net).num("net", conn.net)
            .str("exception", e.getClass().getSimpleName()).end());
        continue;
      }
      ShapeSearchTree tree = board.searchTreeManager.getAutorouteTree(c.traceClearanceClassIndex);
      DestinationDistance d =
          new DestinationDistance(c.traceCosts, c.layerActive, c.minNormalViaCost,
              c.minCheapViaCost);
      // MazeSearchEngine.init: every destination item in set order, every tree shape's box.
      int joined = 0;
      for (Item item : conn.dest) {
        for (int i = 0; i < item.treeShapeCount(tree); i++) {
          TileShape shape = item.getTreeShape(tree, i);
          if (shape != null) {
            d.join(shape.boundingBox(), item.shapeLayer(i));
            joined++;
          }
        }
      }
      out.line(new Line().str("stage", "conn:" + conn.net).num("net", conn.net)
          .num("item", conn.item.getId()).num("joined", joined).end());
      Random random = new Random(11);
      double width = bb.ur.x - bb.ll.x;
      double height = bb.ur.y - bb.ll.y;
      for (int i = 0; i < DISTANCE_SAMPLES; i++) {
        FloatPoint p =
            new FloatPoint(bb.ll.x + random.nextDouble() * width,
                bb.ll.y + random.nextDouble() * height);
        double[] r = new double[layers];
        for (int layer = 0; layer < layers; layer++) {
          r[layer] = d.calculate(p, layer);
        }
        out.line(new Line().dbl("x", p.x).dbl("y", p.y).raw("d", bitsList(r)).end());
      }
      int xBound = bb.ur.x - bb.ll.x + 1;
      int yBound = bb.ur.y - bb.ll.y + 1;
      for (int i = 0; i < DISTANCE_SAMPLES; i++) {
        int x1 = bb.ll.x + random.nextInt(xBound);
        int y1 = bb.ll.y + random.nextInt(yBound);
        int x2 = bb.ll.x + random.nextInt(xBound);
        int y2 = bb.ll.y + random.nextInt(yBound);
        IntBox box =
            new IntBox(Math.min(x1, x2), Math.min(y1, y2), Math.max(x1, x2), Math.max(y1, y2));
        double[] r = new double[layers];
        double[] cheap = new double[layers];
        for (int layer = 0; layer < layers; layer++) {
          r[layer] = d.calculate(box, layer);
          cheap[layer] = d.calculateCheapDistance(box, layer);
        }
        out.line(new Line().raw("box", intArray(new int[] {box.ll.x, box.ll.y, box.ur.x, box.ur.y}))
            .raw("d", bitsList(r)).raw("cheap", bitsList(cheap)).end());
      }
    }
  }

  // ------------------------------------------------------------------------------------------
  // start-rooms (A02.7) and door-sections (A02.5)
  // ------------------------------------------------------------------------------------------

  /** A private field of {@code target}'s class (or a superclass) by reflection. */
  @SuppressWarnings("unchecked")
  static <T> T privateField(Object target, String name) throws Exception {
    Class<?> type = target.getClass();
    while (type != null) {
      try {
        java.lang.reflect.Field field = type.getDeclaredField(name);
        field.setAccessible(true);
        return (T) field.get(target);
      } catch (NoSuchFieldException e) {
        type = type.getSuperclass();
      }
    }
    throw new NoSuchFieldException(name);
  }

  /** A shape: its kind and coordinates ([] for null; a simplex by line count, id and box). */
  static String shapeJson(TileShape shape) {
    if (shape == null) {
      return "[]";
    }
    if (shape instanceof app.freerouting.geometry.planar.IntOctagon o) {
      return "[\"oct\"," + o.leftX + "," + o.bottomY + "," + o.rightX + "," + o.topY + ","
          + o.upperLeftDiagonalX + "," + o.lowerRightDiagonalX + "," + o.lowerLeftDiagonalX + ","
          + o.upperRightDiagonalX + "]";
    }
    if (shape instanceof IntBox b) {
      return "[\"box\"," + b.ll.x + "," + b.ll.y + "," + b.ur.x + "," + b.ur.y + "]";
    }
    IntBox b = shape.boundingBox();
    return "[\"simplex\"," + shape.borderLineCount() + "," + shape.getId() + "," + b.ll.x + ","
        + b.ll.y + "," + b.ur.x + "," + b.ur.y + "]";
  }

  static String boxJson(TileShape shape) {
    IntBox b = shape.boundingBox();
    return "[" + b.ll.x + "," + b.ll.y + "," + b.ur.x + "," + b.ur.y + "]";
  }

  static String roomKind(app.freerouting.autoroute.expansion.ExpansionRoom room) {
    if (room instanceof app.freerouting.autoroute.expansion.CompleteFreeSpaceExpansionRoom) {
      return "free";
    }
    if (room instanceof ObstacleExpansionRoom) {
      return "obstacle";
    }
    if (room instanceof IncompleteFreeSpaceExpansionRoom) {
      return "incomplete";
    }
    return "?";
  }

  /** The tree's walk hash as TreeProbe computes it (probes/board/TreeProbe.java). */
  static String treeLine(String key, ShapeSearchTree tree) throws Exception {
    String treeNode = "app.freerouting.datastructures.ShapeTree$TreeNode";
    java.lang.reflect.Field rootField =
        app.freerouting.datastructures.ShapeTree.class.getDeclaredField("root");
    rootField.setAccessible(true);
    java.lang.reflect.Field boundsField = Class.forName(treeNode).getDeclaredField("boundingShape");
    boundsField.setAccessible(true);
    long hash = FNV_OFFSET;
    int nodes = 0;
    int leaves = 0;
    int maxDepth = 0;
    java.util.Deque<Object[]> stack = new java.util.ArrayDeque<>();
    Object root = rootField.get(tree);
    if (root != null) {
      stack.push(new Object[] {root, 0});
    }
    while (!stack.isEmpty()) {
      Object[] top = stack.pop();
      Object node = top[0];
      int depth = (Integer) top[1];
      nodes++;
      maxDepth = Math.max(maxDepth, depth);
      StringBuilder line = new StringBuilder().append(depth);
      app.freerouting.geometry.planar.RegularTileShape shape =
          (app.freerouting.geometry.planar.RegularTileShape) boundsField.get(node);
      StringBuilder bounds = new StringBuilder();
      if (shape instanceof app.freerouting.geometry.planar.IntOctagon o) {
        bounds.append("O ").append(o.leftX).append(' ').append(o.bottomY).append(' ')
            .append(o.rightX).append(' ').append(o.topY).append(' ').append(o.upperLeftDiagonalX)
            .append(' ').append(o.lowerRightDiagonalX).append(' ').append(o.lowerLeftDiagonalX)
            .append(' ').append(o.upperRightDiagonalX);
      } else if (shape instanceof IntBox b) {
        bounds.append("B ").append(b.ll.x).append(' ').append(b.ll.y).append(' ').append(b.ur.x)
            .append(' ').append(b.ur.y);
      } else {
        bounds.append("? ").append(shape);
      }
      if (node instanceof app.freerouting.datastructures.ShapeTree.Leaf leaf) {
        leaves++;
        line.append(" L ").append(bounds).append(' ');
        if (leaf.object
            instanceof app.freerouting.autoroute.expansion.CompleteFreeSpaceExpansionRoom room) {
          line.append("R ").append(room.getId());
        } else if (leaf.object instanceof Item item) {
          line.append("I ").append(item.getId());
        } else {
          line.append("? ").append(leaf.object);
        }
        line.append(' ').append(leaf.shapeIndexInObject);
      } else {
        app.freerouting.datastructures.ShapeTree.InnerNode inner =
            (app.freerouting.datastructures.ShapeTree.InnerNode) node;
        line.append(" I ").append(bounds);
        stack.push(new Object[] {inner.secondChild, depth + 1});
        stack.push(new Object[] {inner.firstChild, depth + 1});
      }
      line.append('\n');
      for (byte b : line.toString().getBytes(StandardCharsets.UTF_8)) {
        hash ^= (b & 0xff);
        hash *= FNV_PRIME;
      }
    }
    return new Line().str(key, tree.key).num("nodes", nodes).num("leaves", leaves)
        .num("maxDepth", maxDepth).str("shapeHash", hex(hash)).end();
  }

  /** The start rooms of one connection after MazeSearchEngine.getInstance. */
  static void startRoomLines(Out out, String stage, Conn conn, AutorouteEngine engine,
      MazeSearchEngine search) throws Exception {
    List<app.freerouting.autoroute.expansion.CompleteFreeSpaceExpansionRoom> complete =
        privateField(engine, "completeExpansionRooms");
    List<IncompleteFreeSpaceExpansionRoom> incomplete =
        privateField(engine, "incompleteExpansionRooms");
    int count = privateField(engine, "expansionRoomInstanceCount");
    if (complete == null) {
      complete = List.of();
    }
    if (incomplete == null) {
      incomplete = List.of();
    }
    out.line(new Line().str("stage", stage).num("net", conn.net).bool("ok", search != null)
        .num("roomCount", count).num("complete", complete.size())
        .num("incomplete", incomplete.size())
        .num("queue", search == null ? 0 : search.mazeExpansionList.size()).end());
    for (app.freerouting.autoroute.expansion.CompleteFreeSpaceExpansionRoom room : complete) {
      out.line(new Line().num("room", room.getId()).num("layer", room.getLayer())
          .raw("shape", shapeJson(room.getShape())).bool("netDependent", room.isNetDependent())
          .num("doors", room.getDoors().size()).num("targets", room.getTargetDoors().size())
          .end());
      for (app.freerouting.autoroute.expansion.ExpansionDoor door : room.getDoors()) {
        app.freerouting.autoroute.expansion.ExpansionRoom other = door.otherRoom(
            (app.freerouting.autoroute.expansion.ExpansionRoom) room);
        out.line(new Line().num("door", door.getId()).num("dim", door.dimension)
            .str("other", roomKind(other)).num("otherId", other.getId())
            .bool("first", door.firstRoom == room).raw("bounds", boxJson(door.getShape()))
            .end());
      }
      for (app.freerouting.autoroute.expansion.TargetItemExpansionDoor door :
          room.getTargetDoors()) {
        out.line(new Line().num("target", door.getId()).num("item", door.item.getId())
            .num("entry", door.treeEntryNo).bool("destination", door.isDestinationDoor())
            .raw("bounds", boxJson(door.getShape())).end());
      }
    }
    for (IncompleteFreeSpaceExpansionRoom room : incomplete) {
      out.line(new Line().raw("incomplete", shapeJson(room.getShape()))
          .raw("contained", shapeJson(room.getContainedShape())).num("layer", room.getLayer())
          .num("doors", room.getDoors().size()).end());
    }
    if (search != null) {
      for (MazeListElement element : search.mazeExpansionList) {
        out.line(new Line()
            .raw("queue", "[" + quoted(bits(element.sortingValue)) + ","
                + quoted(bits(element.expansionValue)) + "," + element.door.getId() + ","
                + element.sectionNoOfDoor + "]")
            .str("kind", element.door.getClass().getSimpleName())
            .raw("entry", "[" + quoted(bits(element.shapeEntry.a.x)) + ","
                + quoted(bits(element.shapeEntry.a.y)) + "]")
            .num("nextRoom", element.nextRoom == null ? 0 : element.nextRoom.getId()).end());
      }
    }
  }

  static void startRooms(Out out, Loaded l) throws Exception {
    RoutingBoard board = l.board;
    List<Conn> conns = sample(board);
    sampleLine(out, board, conns);
    for (String variant : new String[] {"off", "on"}) {
      boolean ripup = variant.equals("on");
      for (Conn conn : conns) {
        String stage = variant + ":" + conn.net;
        if (!conn.state.isEmpty()) {
          out.line(new Line().str("stage", stage).num("net", conn.net).str("state", conn.state)
              .end());
          continue;
        }
        AutorouteControl c;
        try {
          c = routerControl(l, conn.net, ripup);
        } catch (RuntimeException e) {
          out.line(new Line().str("stage", stage).num("net", conn.net)
              .str("exception", e.getClass().getSimpleName()).end());
          continue;
        }
        AutorouteEngine engine = new AutorouteEngine(board, c.traceClearanceClassIndex, false);
        engine.initConnection(conn.net, null, null);
        MazeSearchEngine search;
        try {
          search = MazeSearchEngine.getInstance(conn.start, conn.dest, engine, c);
        } catch (Exception e) {
          out.line(new Line().str("stage", stage).num("net", conn.net)
              .str("exception", e.getClass().getSimpleName()).end());
          engine.clear();
          out.line(treeLine("cleared", engine.autorouteSearchTree));
          continue;
        }
        startRoomLines(out, stage, conn, engine, search);
        engine.clear();
        out.line(treeLine("cleared", engine.autorouteSearchTree));
      }
    }
  }

  static String segmentsJson(app.freerouting.geometry.planar.FloatLine[] segments) {
    StringBuilder text = new StringBuilder("[");
    for (int i = 0; i < segments.length; i++) {
      if (i > 0) {
        text.append(',');
      }
      app.freerouting.geometry.planar.FloatLine s = segments[i];
      text.append('[').append(quoted(bits(s.a.x))).append(',').append(quoted(bits(s.a.y)))
          .append(',').append(quoted(bits(s.b.x))).append(',').append(quoted(bits(s.b.y)))
          .append(']');
    }
    return text.append(']').toString();
  }

  static String doorSectionLine(app.freerouting.autoroute.expansion.ExpansionDoor door,
      double offset) {
    app.freerouting.geometry.planar.FloatLine[] segments = door.getSectionSegments(offset);
    return new Line().num("door", door.getId()).num("dim", door.dimension)
        .dbl("offset", offset).num("count", segments.length)
        .num("sections", door.sectionArr == null ? 0 : door.sectionArr.length)
        .raw("segments", segmentsJson(segments)).end();
  }

  static final int DOOR_SECTION_DOORS = 100;

  static void doorSections(Out out, Loaded l) throws Exception {
    // A hand-built pair: room b overlaps room a (a 2-dimensional door between free rooms),
    // room c touches a along x = 0 (a 1-dimensional door).
    app.freerouting.autoroute.expansion.CompleteFreeSpaceExpansionRoom a =
        new app.freerouting.autoroute.expansion.CompleteFreeSpaceExpansionRoom(
            new IntBox(0, 0, 1000, 1000).boundingOctagon(), 0, 1);
    app.freerouting.autoroute.expansion.CompleteFreeSpaceExpansionRoom b =
        new app.freerouting.autoroute.expansion.CompleteFreeSpaceExpansionRoom(
            new IntBox(500, 200, 2000, 800).boundingOctagon(), 0, 2);
    app.freerouting.autoroute.expansion.CompleteFreeSpaceExpansionRoom cRoom =
        new app.freerouting.autoroute.expansion.CompleteFreeSpaceExpansionRoom(
            new IntBox(-800, 0, 0, 600).boundingOctagon(), 0, 3);
    app.freerouting.autoroute.expansion.ExpansionDoor ab =
        new app.freerouting.autoroute.expansion.ExpansionDoor(a, b);
    app.freerouting.autoroute.expansion.ExpansionDoor ac =
        new app.freerouting.autoroute.expansion.ExpansionDoor(a, cRoom);
    for (double offset : new double[] {0, 10, 50, 200}) {
      out.line(new Line().str("stage", "hand:ab:" + offset).end());
      out.line(doorSectionLine(ab, offset));
      out.line(new Line().str("stage", "hand:ac:" + offset).end());
      out.line(doorSectionLine(ac, offset));
    }
    // The first 100 doors of the routable sampled connections (ripup off), in room list order.
    RoutingBoard board = l.board;
    int printed = 0;
    for (Conn conn : sample(board)) {
      if (printed >= DOOR_SECTION_DOORS) {
        break;
      }
      if (!conn.state.isEmpty()) {
        continue;
      }
      AutorouteControl c;
      try {
        c = routerControl(l, conn.net, false);
      } catch (RuntimeException e) {
        continue;
      }
      AutorouteEngine engine = new AutorouteEngine(board, c.traceClearanceClassIndex, false);
      engine.initConnection(conn.net, null, null);
      MazeSearchEngine.getInstance(conn.start, conn.dest, engine, c);
      List<app.freerouting.autoroute.expansion.CompleteFreeSpaceExpansionRoom> complete =
          privateField(engine, "completeExpansionRooms");
      List<app.freerouting.autoroute.expansion.ExpansionDoor> doors = new ArrayList<>();
      for (app.freerouting.autoroute.expansion.CompleteFreeSpaceExpansionRoom room :
          complete == null
              ? List.<app.freerouting.autoroute.expansion.CompleteFreeSpaceExpansionRoom>of()
              : complete) {
        for (app.freerouting.autoroute.expansion.ExpansionDoor door : room.getDoors()) {
          boolean seen = false;
          for (app.freerouting.autoroute.expansion.ExpansionDoor d : doors) {
            seen |= d == door;
          }
          if (!seen && printed + doors.size() < DOOR_SECTION_DOORS) {
            doors.add(door);
          }
        }
      }
      out.line(new Line().str("stage", "conn:" + conn.net).num("net", conn.net)
          .num("doors", doors.size()).end());
      for (app.freerouting.autoroute.expansion.ExpansionDoor door : doors) {
        out.line(doorSectionLine(door, c.compensatedTraceHalfWidth[door.firstRoom.getLayer()]));
      }
      printed += doors.size();
      engine.clear();
    }
  }

  // ------------------------------------------------------------------------------------------
  // drills (A03.5)
  // ------------------------------------------------------------------------------------------

  /** A point: [x,y] for an IntPoint, else its text. */
  static String pointJson(app.freerouting.geometry.planar.Point point) {
    if (point instanceof app.freerouting.geometry.planar.IntPoint p) {
      return "[" + p.x + "," + p.y + "]";
    }
    return quoted(String.valueOf(point));
  }

  /** The drills of every page of the engine, row by row, as a real search would see them. */
  static void drillLines(Out out, AutorouteEngine engine, boolean attachSmd) throws Exception {
    java.lang.reflect.Field pagesField =
        app.freerouting.autoroute.drill.DrillPageArray.class.getDeclaredField("pages");
    pagesField.setAccessible(true);
    DrillPage[][] pages = (DrillPage[][]) pagesField.get(engine.drillPageArray);
    for (DrillPage[] row : pages) {
      for (DrillPage page : row) {
        Collection<ExpansionDrill> drills = page.getDrills(engine, attachSmd);
        out.line(new Line().num("page", page.getId()).raw("bounds", boxJson(page.shape))
            .num("drills", drills.size()).end());
        for (ExpansionDrill drill : drills) {
          StringBuilder rooms = new StringBuilder("[");
          for (int i = 0; i < drill.roomArr.length; i++) {
            if (i > 0) {
              rooms.append(',');
            }
            rooms.append(drill.roomArr[i] == null ? "null" : drill.roomArr[i].getId());
          }
          rooms.append(']');
          out.line(new Line().num("drill", drill.getId()).raw("location", pointJson(drill.location))
              .raw("shape", shapeJson(drill.getShape())).num("first", drill.firstLayer)
              .num("last", drill.lastLayer).raw("rooms", rooms.toString()).end());
        }
      }
    }
  }

  /**
   * The variants of the drills kind: ripup off, ripup on, and "smd" (ripup off with attachSmd
   * forced on, so the own-net SMD pin rules of getDrills are compared on boards whose via rules
   * all say attach off).
   */
  static void drills(Out out, Loaded l) throws Exception {
    RoutingBoard board = l.board;
    List<Conn> conns = sample(board);
    sampleLine(out, board, conns);
    for (String variant : new String[] {"off", "on", "smd"}) {
      boolean ripup = variant.equals("on");
      for (Conn conn : conns) {
        String stage = variant + ":" + conn.net;
        if (!conn.state.isEmpty()) {
          out.line(new Line().str("stage", stage).num("net", conn.net).str("state", conn.state)
              .end());
          continue;
        }
        AutorouteControl c;
        try {
          c = routerControl(l, conn.net, ripup);
        } catch (RuntimeException e) {
          out.line(new Line().str("stage", stage).num("net", conn.net)
              .str("exception", e.getClass().getSimpleName()).end());
          continue;
        }
        AutorouteEngine engine = new AutorouteEngine(board, c.traceClearanceClassIndex, false);
        engine.initConnection(conn.net, null, null);
        MazeSearchEngine search;
        try {
          search = MazeSearchEngine.getInstance(conn.start, conn.dest, engine, c);
        } catch (Exception e) {
          out.line(new Line().str("stage", stage).num("net", conn.net)
              .str("exception", e.getClass().getSimpleName()).end());
          engine.clear();
          out.line(treeLine("cleared", engine.autorouteSearchTree));
          continue;
        }
        boolean attachSmd = variant.equals("smd") || c.attachSmdAllowed;
        out.line(new Line().str("stage", stage).num("net", conn.net).bool("ok", search != null)
            .bool("attachSmd", attachSmd).end());
        if (search != null) {
          drillLines(out, engine, attachSmd);
        }
        List<app.freerouting.autoroute.expansion.CompleteFreeSpaceExpansionRoom> complete =
            privateField(engine, "completeExpansionRooms");
        int count = privateField(engine, "expansionRoomInstanceCount");
        out.line(new Line().num("roomCount", count)
            .num("complete", complete == null ? 0 : complete.size()).end());
        engine.clear();
        out.line(treeLine("cleared", engine.autorouteSearchTree));
      }
    }
  }

  // ------------------------------------------------------------------------------------------
  // maze-log (A04.8)
  // ------------------------------------------------------------------------------------------

  /** At most this many occupyNextElement calls per connection (both sides). */
  static final int MAZE_MAX_STEPS = 200000;

  static String expandableKind(app.freerouting.autoroute.expansion.ExpandableObject door) {
    return door == null ? "null" : door.getClass().getSimpleName();
  }

  static String adjustmentName(MazeSearchElement.Adjustment adjustment) {
    return adjustment == null ? "null" : adjustment.name();
  }

  /** The occupation of section {@code section} of {@code door}; false out of range. */
  static boolean sectionOccupied(app.freerouting.autoroute.expansion.ExpandableObject door,
      int section) {
    return section < door.mazeSearchElementCount()
        && door.getMazeSearchElement(section).isOccupied;
  }

  static long workUnits() {
    return app.freerouting.datastructures.WorkMeter.current().units();
  }

  /** Steps one search with occupyNextElement and prints every step, then the end. */
  static void mazeSteps(Out out, AutorouteEngine engine, MazeSearchEngine search, long start)
      throws Exception {
    int step = 0;
    boolean more = true;
    while (more && step < MAZE_MAX_STEPS) {
      int skipped = 0;
      MazeListElement next = null;
      for (MazeListElement element : search.mazeExpansionList) {
        if (!element.door.getMazeSearchElement(element.sectionNoOfDoor).isOccupied) {
          next = element;
          break;
        }
        skipped++;
      }
      Line line = new Line().num("step", step);
      if (next != null) {
        line.str("kind", expandableKind(next.door)).num("door", next.door.getId())
            .num("section", next.sectionNoOfDoor)
            .raw("key", "[" + quoted(bits(next.sortingValue)) + ","
                + quoted(bits(next.expansionValue)) + "]");
        if (next.nextRoom == null) {
          line.raw("room", "[]");
        } else {
          line.raw("room", "[" + quoted(next.nextRoom.getClass().getSimpleName()) + ","
              + next.nextRoom.getId() + "," + next.nextRoom.getLayer() + "]");
        }
        if (next.backtrackDoor == null) {
          line.raw("back", "[]");
        } else {
          line.raw("back", "[" + quoted(expandableKind(next.backtrackDoor)) + ","
              + next.backtrackDoor.getId() + "," + next.sectionNoOfBacktrackDoor + "]");
        }
        line.bool("checked", next.alreadyChecked).str("adjust", adjustmentName(next.adjustment))
            .bool("ripped", next.roomRipped).num("ripup", next.ripupCost);
      }
      more = search.occupyNextElement();
      line.num("skipped", skipped);
      if (next != null) {
        line.bool("occupied", sectionOccupied(next.door, next.sectionNoOfDoor));
      }
      line.num("units", workUnits() - start);
      out.line(line.end());
      step++;
    }
    app.freerouting.autoroute.expansion.ExpandableObject destination =
        privateField(search, "destinationDoor");
    int destinationSection = privateField(search, "sectionNoOfDestinationDoor");
    int count = privateField(engine, "expansionRoomInstanceCount");
    Line end = new Line().num("steps", step).bool("capped", more);
    if (destination == null) {
      end.raw("destination", "[]");
    } else {
      end.raw("destination", "[" + quoted(expandableKind(destination)) + ","
          + destination.getId() + "," + destinationSection + "]");
    }
    out.line(end.num("roomCount", count).num("queue", search.mazeExpansionList.size())
        .num("units", workUnits() - start).end());
  }

  static void mazeLog(Out out, Loaded l) throws Exception {
    RoutingBoard board = l.board;
    List<Conn> conns = sample(board);
    sampleLine(out, board, conns);
    for (String variant : new String[] {"off", "on"}) {
      for (Conn conn : conns) {
        mazeConnection(out, l, conn, variant);
      }
    }
  }

  /** One connection of maze-log and maze-routed in one variant (ripup off or on). */
  static void mazeConnection(Out out, Loaded l, Conn conn, String variant) throws Exception {
    RoutingBoard board = l.board;
    boolean ripup = variant.equals("on");
    String stage = variant + ":" + conn.net;
    if (!conn.state.isEmpty()) {
      out.line(new Line().str("stage", stage).num("net", conn.net).str("state", conn.state)
          .end());
      return;
    }
    AutorouteControl c;
    try {
      c = routerControl(l, conn.net, ripup);
    } catch (RuntimeException e) {
      out.line(new Line().str("stage", stage).num("net", conn.net)
          .str("exception", e.getClass().getSimpleName()).end());
      return;
    }
    AutorouteEngine engine = new AutorouteEngine(board, c.traceClearanceClassIndex, false);
    engine.initConnection(conn.net, null, null);
    long start = workUnits();
    MazeSearchEngine search;
    try {
      search = MazeSearchEngine.getInstance(conn.start, conn.dest, engine, c);
      out.line(new Line().str("stage", stage).num("net", conn.net).bool("ok", search != null)
          .num("units", workUnits() - start).end());
      if (search != null) {
        mazeSteps(out, engine, search, start);
      }
    } catch (Exception e) {
      out.line(new Line().str("exception", e.getClass().getSimpleName()).end());
    }
    engine.clear();
    out.line(treeLine("cleared", engine.autorouteSearchTree));
  }

  /** At most this many nets are cut and searched per board by maze-routed. */
  static final int MAZE_ROUTED_NETS = 12;

  static boolean isRouteItem(Item item) {
    return item instanceof app.freerouting.board.model.items.Trace
        || item instanceof app.freerouting.board.model.items.Via;
  }

  /**
   * maze-routed (A04.8 for A04.6 and A04.7): the board's own traces and vias are unfixed
   * (Item.unfix), then net after net (up to 12 nets that have route items) the route items of
   * the net are removed in item order and the net's sampled connection is searched as in
   * maze-log, ripup off and on, through the other nets' routes. The removed items stay removed.
   */
  static void mazeRouted(Out out, Loaded l) throws Exception {
    RoutingBoard board = l.board;
    List<Integer> unfixed = new ArrayList<>();
    for (Item item : board.getItems()) {
      if (isRouteItem(item)) {
        item.unfix();
        unfixed.add(item.getId());
      }
    }
    out.line(new Line().str("stage", "unfix").raw("items", ints(unfixed)).end());
    int done = 0;
    int maxNet = board.rules.nets.maxNetNumber();
    for (int net = 1; net <= maxNet && done < MAZE_ROUTED_NETS; net++) {
      List<Item> routed = new ArrayList<>();
      for (Item item : board.getItems()) {
        if (isRouteItem(item) && item.containsNet(net)) {
          routed.add(item);
        }
      }
      if (routed.isEmpty()) {
        continue;
      }
      List<Integer> removed = new ArrayList<>();
      for (Item item : routed) {
        board.removeItem(item);
        removed.add(item.getId());
      }
      Conn conn = null;
      for (Conn candidate : sample(board)) {
        if (candidate.net == net) {
          conn = candidate;
        }
      }
      out.line(new Line().str("stage", "cut:" + net).raw("removed", ints(removed))
          .bool("connection", conn != null).end());
      if (conn == null) {
        continue;
      }
      done++;
      for (String variant : new String[] {"off", "on"}) {
        mazeConnection(out, l, conn, variant);
      }
    }
  }

  // ------------------------------------------------------------------------------------------
  // The loader: J01's route mode (DefaultSettings + DsnFileSettings, then RoutingJobScheduler's
  // load sequence call for call, without starting the router thread)
  // ------------------------------------------------------------------------------------------

  static final class Loaded {
    RoutingJob job;
    RoutingBoard board;

    static Loaded load(String dsn) throws Exception {
      Freerouting.globalSettings = new GlobalSettings();
      GlobalSettings gs = Freerouting.globalSettings;
      gs.settingsMergerProtype = new SettingsMerger(new DefaultSettings());
      RoutingJob job = new RoutingJob(new UUID(0L, 1L));
      job.setInput(new File(dsn));
      // CliRunner.initializeCli: the first merge.
      SettingsMerger first = gs.settingsMergerProtype.clone();
      first.addOrReplaceSources(new DsnFileSettings(job.input.getData(), job.input.getFilename()));
      job.routerSettings = first.merge();
      job.drcSettings = gs.drcSettings.clone();
      // RoutingJobScheduler: the load, with the router settings applied to the loaded board.
      HeadlessBoardManager manager = new HeadlessBoardManager(job);
      manager.loadFromSpecctraDsn(job.input.getData(), null, new ItemIdGenerator());
      job.board = manager.getRoutingBoard();
      if (job.board == null) {
        throw new IllegalStateException("the board did not load");
      }
      job.board.awaitPostLoad();
      // RoutingJobScheduler: the second merge with the adjacent rules file, if any.
      SettingsMerger second = gs.settingsMergerProtype.clone();
      second.addOrReplaceSources(new DsnFileSettings(job.input.getData(), job.input.getFilename()));
      byte[] rules = null;
      if (job.input.getDirectoryPath() != null) {
        String base = job.input.getFilename();
        if (base.lastIndexOf('.') > 0) {
          base = base.substring(0, base.lastIndexOf('.'));
        }
        File adjacent = new File(job.input.getDirectoryPath(), base + ".rules");
        if (adjacent.exists()) {
          rules = Files.readAllBytes(adjacent.toPath());
          second.addOrReplaceSources(
              new RulesFileSettings(new ByteArrayInputStream(rules), adjacent.getName()));
        }
      }
      second.addOrReplaceSources(new ApiSettings(job.routerSettings));
      job.routerSettings = second.merge();
      if (rules != null) {
        RulesReader.read(new ByteArrayInputStream(rules),
            job.name != null ? job.name : "board", job.board, job.routerSettings);
      }
      job.routerSettings.applyBoardSpecificOptimizations(job.board);
      job.routerSettings.applyNetClassExclusions(job.board);
      Loaded l = new Loaded();
      l.job = job;
      l.board = job.board;
      return l;
    }
  }

  // ------------------------------------------------------------------------------------------
  // The writer (crates/freerouting/src/autoroute/probe.rs writes the same text)
  // ------------------------------------------------------------------------------------------

  static final long FNV_OFFSET = 0xcbf29ce484222325L;
  static final long FNV_PRIME = 0x100000001b3L;

  /** FNV-1a 64 over the lines, each followed by '\n'. */
  static long fnvLines(Collection<String> lines) {
    long hash = FNV_OFFSET;
    for (String line : lines) {
      for (byte b : (line + "\n").getBytes(StandardCharsets.UTF_8)) {
        hash ^= (b & 0xff);
        hash *= FNV_PRIME;
      }
    }
    return hash;
  }

  static String hex(long value) {
    return String.format("%016x", value);
  }

  static String bits(double value) {
    return "0x" + String.format("%016x", Double.doubleToRawLongBits(value));
  }

  static String quoted(String text) {
    return "\"" + text + "\"";
  }

  static String ids(Collection<Item> items) {
    StringBuilder text = new StringBuilder("[");
    for (Item item : items) {
      if (text.length() > 1) {
        text.append(',');
      }
      text.append(item.getId());
    }
    return text.append(']').toString();
  }

  static String ints(Collection<Integer> values) {
    StringBuilder text = new StringBuilder("[");
    for (int value : values) {
      if (text.length() > 1) {
        text.append(',');
      }
      text.append(value);
    }
    return text.append(']').toString();
  }

  static String intArray(int[] values) {
    StringBuilder text = new StringBuilder("[");
    for (int i = 0; i < values.length; i++) {
      if (i > 0) {
        text.append(',');
      }
      text.append(values[i]);
    }
    return text.append(']').toString();
  }

  static String boolArray(boolean[] values) {
    StringBuilder text = new StringBuilder("[");
    for (int i = 0; i < values.length; i++) {
      if (i > 0) {
        text.append(',');
      }
      text.append(values[i]);
    }
    return text.append(']').toString();
  }

  static String bitsList(double[] values) {
    StringBuilder text = new StringBuilder("[");
    for (int i = 0; i < values.length; i++) {
      if (i > 0) {
        text.append(',');
      }
      text.append(quoted(bits(values[i])));
    }
    return text.append(']').toString();
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

    Out(String kind, String board) {
      line(new Line().str("kind", kind).str("board", board)
          .str("javaCommit", app.freerouting.constants.Constants.FREEROUTING_VERSION)
          .num("format", 1).end());
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
