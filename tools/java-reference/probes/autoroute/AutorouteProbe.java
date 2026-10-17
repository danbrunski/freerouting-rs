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
 *   <li>locate (A05.5): per sampled connection and variant, findConnection, then
 *       FoundConnectionLocator.getInstance as autorouteConnection calls it: the backtrack array
 *       (door kind and id, section, next room), the result items (corners, layer), the ripped
 *       ids in TreeSet order with their costs in LinkedHashMap order, the start and target items
 *       and layers; {"result":"none"} when the search fails. Then the tree's walk hash after
 *       clear().
 *   <li>locate-routed (A05.5): locate through pre-routed traces, as maze-routed walks them.
 *   <li>insert (A06.7): per sampled connection, on a fresh load of the board, the control as the
 *       batch router builds it in pass 1 (ripup on, the start ripup costs) and
 *       AutorouteEngine.autorouteConnection on a new engine with the router's maze-search
 *       WorkLimit: the state and details, the ripped ids, every item added, changed (same id,
 *       other geometry) and removed in id order, maxGeneratedId, the BoardStatistics deltas
 *       (incomplete connections, vias, total trace length) and the thread's WorkMeter units the
 *       call used.
 *   <li>insert-routed (A06.7, for the rip of A06.5): maze-routed's walk (traces and vias unfixed,
 *       then for up to 12 nets with route items the net's route items removed and its sampled
 *       connection routed as in insert, through the other nets' routes, on the same board, which
 *       keeps what each connection inserted and ripped).
 *   <li>pass (A11.7): passes 1 and 2 of a BatchAutorouter(job) on the loaded board (no fanout),
 *       by replicating AutoroutePassRunner.runSingleThread's item loop around the real
 *       package-private autorouteItem, with Java's own getAutorouteItems and
 *       reorderSingleThreadItems (reflection). Per pass the item list; per connection the item,
 *       net, state, ripped ids, maxGeneratedId before and after, the count and FNV-1a 64 of the
 *       added traces and vias (itemFields lines, id order), the FNV-1a 64 of every trace and via,
 *       the item's failure count and the thread's WorkMeter units from before autorouteItem to
 *       after the failure handling. Per pass the tail removal (its result and units), the
 *       routing ledger's incomplete count and nets, the pass result and the board hash. Then the
 *       real autoroutePass(1) and (2) on a second fresh load, and whether each pass left the
 *       same board hash as the replica's.
 *   <li>fanout (A12.6): fanout pass 0 (BatchFanout.fanoutPass) on the loaded board, by
 *       replicating its pin loop around the real RoutingBoard.fanout and
 *       BatchAutorouter.enforceStrictDrc, with BatchFanout's own component and pin order
 *       (reflection on a BatchFanout built by its private constructor). Per component a stage
 *       line; per pin the component and pin ids, the net, the skip reason or the state, whether
 *       the strict DRC rejected it, maxGeneratedId before and after, the count and FNV-1a 64 of
 *       the added traces and vias, the FNV-1a 64 of all traces and vias and the thread's
 *       WorkMeter units from before the pin's WorkLimit to after the strict DRC. Then the pass
 *       counters, and whether the real BatchFanout.fanoutBoard with fanout.maxPasses = 1 on a
 *       second load left the same traces and vias and the same board hash
 *       (replica_matches_real).
 *   <li>fanout-cutoff (A12.6): fanout with fanout.maxMillisecondsPerPin = 1, so the per-pin
 *       WorkLimit fires.
 *   <li>fanout-frames (A12.5): fanout on Issue558-dev-board with U2 as placed, rotated 90
 *       degrees and mirrored to the back (crates/freerouting/tests/data/fanout-frames/), then
 *       U2's SMD pins with a net and how many of them are escaped (isPinEscaped) after the
 *       replica's pass.
 *   <li>plane-island (A12.4): fanout on min_fr_test with its GND plane cut into a main plane
 *       and a same-net island under both GND pads (crates/freerouting/tests/data/plane-island/),
 *       then per GND pin the conduction areas in its connected set, and the board's incomplete
 *       count (BoardStatistics).
 *   <li>last-mile (A13.3): LastMileBlockerRipup.ripBlockers on the loaded board after importing
 *       Java's routed reference SES (fixtures/reference/route/<board>.ses, SesReader.read) and
 *       unfixing its traces and vias (SesReader inserts them user fixed). Per airline of the DRC
 *       (getAllAirlines, in order) its net, items and corners, the corridor (the class's own
 *       corridor, by reflection) and the overlapping items in TreeSet order, and the blockers it
 *       added (isRemovableBlocker, by reflection) while the set holds fewer than MAX_ITEMS; then
 *       the blockers in order. Then the real ripBlockers on a second import, its count, and
 *       whether it left the same item ids as the replica's removeItems (replica_matches_real).
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
        case "locate" -> locate(out, Loaded.load(dsn));
        case "locate-routed" -> locateRouted(out, Loaded.load(dsn));
        case "insert" -> insert(out, dsn);
        case "insert-routed" -> insertRouted(out, Loaded.load(dsn));
        case "pass" -> pass(out, dsn);
        case "treemap" -> treeMap(out);
        case "fanout" -> fanout(out, dsn, null, null);
        case "fanout-cutoff" -> fanout(out, dsn, 1L, null);
        case "fanout-frames" -> fanout(out, dsn, null, "U2");
        case "plane-island" -> planeIsland(out, dsn);
        case "last-mile" -> lastMile(out, dsn);
        default -> throw new IllegalArgumentException("unknown kind " + kind);
      }
    } catch (Throwable t) {
      out.exception(t);
    }
    out.write(args[2]);
  }

  // ------------------------------------------------------------------------------------------
  // treemap (A07.3)
  // ------------------------------------------------------------------------------------------

  static final int TREEMAP_SEQUENCES = 200;
  static final int TREEMAP_POINTS = 50;
  static final int TREEMAP_RANGE = 32;

  /** A point of the treemap probe: its index in the sequence and its distance from 0. */
  record TreePoint(int index, int x, int y, double dist) {}

  /**
   * Not transitive, the shape of SortedRoomNeighbours.SortedRoomNeighbour.compareTo: distances
   * within 1.0 count as equal and fall through to the y difference, then the x difference.
   */
  static int treeCompare(TreePoint a, TreePoint b) {
    double delta = a.dist() - b.dist();
    if (Math.abs(delta) <= 1.0) {
      delta = a.y() - b.y();
    }
    int res = delta > 0 ? 1 : delta < 0 ? -1 : 0;
    if (res == 0) {
      res = a.x() - b.x();
    }
    return res;
  }

  /**
   * 200 sequences of 50 points (x and y from nextInt(32) of one new Random(13)) into a TreeSet
   * with treeCompare: per sequence the points, whether each add added it, and the iteration
   * order as point indexes.
   */
  static void treeMap(Out out) {
    Random random = new Random(13);
    for (int seq = 0; seq < TREEMAP_SEQUENCES; seq++) {
      java.util.TreeSet<TreePoint> set = new java.util.TreeSet<>(AutorouteProbe::treeCompare);
      List<Integer> coords = new ArrayList<>();
      boolean[] added = new boolean[TREEMAP_POINTS];
      for (int i = 0; i < TREEMAP_POINTS; i++) {
        int x = random.nextInt(TREEMAP_RANGE);
        int y = random.nextInt(TREEMAP_RANGE);
        coords.add(x);
        coords.add(y);
        added[i] = set.add(new TreePoint(i, x, y, Math.sqrt((double) x * x + (double) y * y)));
      }
      List<Integer> order = new ArrayList<>();
      for (TreePoint p : set) {
        order.add(p.index());
      }
      out.line(new Line().str("stage", "treemap").num("seq", seq).raw("points", ints(coords))
          .raw("added", boolArray(added)).raw("order", ints(order)).end());
    }
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
  // locate (A05.5)
  // ------------------------------------------------------------------------------------------

  /** [kind, id, layer] of a room, [] for null. */
  static String roomJson(app.freerouting.autoroute.expansion.ExpansionRoom room) {
    if (room == null) {
      return "[]";
    }
    return "[" + quoted(room.getClass().getSimpleName()) + "," + room.getId() + ","
        + room.getLayer() + "]";
  }

  static String cornersJson(app.freerouting.geometry.planar.IntPoint[] corners) {
    StringBuilder text = new StringBuilder("[");
    for (int i = 0; i < corners.length; i++) {
      if (i > 0) {
        text.append(',');
      }
      text.append('[').append(corners[i].x).append(',').append(corners[i].y).append(']');
    }
    return text.append(']').toString();
  }

  static String costsJson(java.util.Map<Item, Integer> costs) {
    StringBuilder text = new StringBuilder("[");
    for (java.util.Map.Entry<Item, Integer> entry : costs.entrySet()) {
      if (text.length() > 1) {
        text.append(',');
      }
      text.append('[').append(entry.getKey().getId()).append(',').append(entry.getValue())
          .append(']');
    }
    return text.append(']').toString();
  }

  /**
   * locate: findConnection, then FoundConnectionLocator.getInstance with a TreeSet of ripped items
   * and a LinkedHashMap of costs, as AutorouteEngine.autorouteConnection calls it. Prints the
   * backtrack array, the result items, the ripped ids in set order with the costs in map order,
   * and the start and target items and layers; one {"result":"none"} line if the search failed.
   */
  static void locateLines(Out out, Loaded l, AutorouteEngine engine, AutorouteControl c,
      MazeSearchEngine search) throws Exception {
    MazeSearchEngine.Result result = search.findConnection();
    if (result == null) {
      out.line(new Line().str("result", "none").end());
      return;
    }
    java.util.SortedSet<Item> ripped = new java.util.TreeSet<>();
    java.util.Map<Item, Integer> costs = new java.util.LinkedHashMap<>();
    app.freerouting.autoroute.path.FoundConnectionLocator located;
    try {
      located = app.freerouting.autoroute.path.FoundConnectionLocator.getInstance(result, c,
          engine.autorouteSearchTree, l.board.rules.getTraceAngleRestriction(), ripped, costs);
    } catch (Exception e) {
      out.line(new Line().str("exception", e.getClass().getSimpleName()).raw("ripped", ids(ripped))
          .raw("costs", costsJson(costs)).end());
      return;
    }
    Object[] chain = privateField(located, "backtrackArray");
    for (int i = 0; i < chain.length; i++) {
      app.freerouting.autoroute.expansion.ExpandableObject door = privateField(chain[i], "door");
      int section = privateField(chain[i], "sectionNoOfDoor");
      app.freerouting.autoroute.expansion.CompleteExpansionRoom room =
          privateField(chain[i], "nextRoom");
      out.line(new Line().num("back", i).str("kind", expandableKind(door)).num("door", door.getId())
          .num("section", section).raw("room", roomJson(room)).end());
    }
    int index = 0;
    for (Object item : located.connectionItems) {
      app.freerouting.geometry.planar.IntPoint[] corners = privateField(item, "corners");
      int layer = privateField(item, "layer");
      out.line(new Line().num("item", index).num("layer", layer).raw("corners", cornersJson(corners))
          .end());
      index++;
    }
    out.line(new Line().raw("ripped", ids(ripped)).raw("costs", costsJson(costs)).end());
    out.line(new Line()
        .num("start", located.startItem == null ? -1 : located.startItem.getId())
        .num("target", located.targetItem == null ? -1 : located.targetItem.getId())
        .num("startLayer", located.startLayer).num("targetLayer", located.targetLayer).end());
  }

  /** One connection of locate and locate-routed in one variant (ripup off or on). */
  static void locateConnection(Out out, Loaded l, Conn conn, String variant) throws Exception {
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
    try {
      MazeSearchEngine search = MazeSearchEngine.getInstance(conn.start, conn.dest, engine, c);
      out.line(new Line().str("stage", stage).num("net", conn.net).bool("ok", search != null)
          .end());
      if (search != null) {
        locateLines(out, l, engine, c, search);
      }
    } catch (Exception e) {
      out.line(new Line().str("exception", e.getClass().getSimpleName()).end());
    }
    engine.clear();
    out.line(treeLine("cleared", engine.autorouteSearchTree));
  }

  static void locate(Out out, Loaded l) throws Exception {
    RoutingBoard board = l.board;
    List<Conn> conns = sample(board);
    sampleLine(out, board, conns);
    for (String variant : new String[] {"off", "on"}) {
      for (Conn conn : conns) {
        locateConnection(out, l, conn, variant);
      }
    }
  }

  /**
   * locate-routed (A05.5 for the ripped items of A05.1): mazeRouted's walk (traces and vias
   * unfixed, then for up to 12 nets with route items the net's route items removed and its
   * sampled connection searched through the others' routes), with locateConnection per variant.
   */
  static void locateRouted(Out out, Loaded l) throws Exception {
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
        locateConnection(out, l, conn, variant);
      }
    }
  }

  // ------------------------------------------------------------------------------------------
  // insert (A06.7)
  // ------------------------------------------------------------------------------------------

  /** A corner as [x,y] when integer, else ["r",xbits,ybits] of its float approximation. */
  static String cornerJson(app.freerouting.geometry.planar.Point p) {
    if (p instanceof app.freerouting.geometry.planar.IntPoint ip) {
      return "[" + ip.x + "," + ip.y + "]";
    }
    FloatPoint f = p.toFloat();
    return "[\"r\"," + quoted(bits(f.x)) + "," + quoted(bits(f.y)) + "]";
  }

  static String netsJson(Item item) {
    List<Integer> nets = new ArrayList<>();
    for (int i = 0; i < item.netCount(); i++) {
      nets.add(item.getNetNumber(i));
    }
    return ints(nets);
  }

  /** The fields of an item the insert probe prints after its id and key. */
  static String itemFields(Item item) {
    Line line = new Line();
    if (item instanceof app.freerouting.board.trace.PolylineTrace trace) {
      StringBuilder corners = new StringBuilder("[");
      app.freerouting.geometry.planar.Polyline polyline = trace.polyline();
      for (int i = 0; i < polyline.cornerCount(); i++) {
        if (i > 0) {
          corners.append(',');
        }
        corners.append(cornerJson(polyline.corner(i)));
      }
      corners.append(']');
      line.str("kind", "trace").raw("nets", netsJson(item)).num("layer", trace.getLayer())
          .num("halfWidth", trace.getHalfWidth()).num("class", item.clearanceClassIndex())
          .str("fixed", item.getFixedState().name()).raw("corners", corners.toString());
    } else if (item instanceof app.freerouting.board.model.items.Via via) {
      line.str("kind", "via").raw("nets", netsJson(item)).str("padstack", via.getPadstack().name)
          .raw("center", cornerJson(via.getCenter())).num("class", item.clearanceClassIndex())
          .str("fixed", item.getFixedState().name());
    } else {
      line.str("kind", item.getClass().getSimpleName()).raw("nets", netsJson(item));
    }
    String text = line.end();
    // Drop the braces: the caller prefixes the id.
    return text.substring(1, text.length() - 1);
  }

  /** One sampled connection of insert, on a fresh load (Java autorouteConnection). */
  static void insertConnection(Out out, Loaded l, Conn conn) {
    RoutingBoard board = l.board;
    String stage = "insert:" + conn.net;
    if (!conn.state.isEmpty()) {
      out.line(new Line().str("stage", stage).num("net", conn.net).str("state", conn.state)
          .end());
      return;
    }
    AutorouteControl c;
    try {
      c = routerControl(l, conn.net, true);
    } catch (RuntimeException e) {
      out.line(new Line().str("stage", stage).num("net", conn.net)
          .str("exception", e.getClass().getSimpleName()).end());
      return;
    }
    out.line(new Line().str("stage", stage).num("net", conn.net).raw("start", ids(conn.start))
        .raw("dest", ids(conn.dest)).end());
    java.util.TreeMap<Integer, String> before = new java.util.TreeMap<>();
    for (Item item : board.getItems()) {
      before.put(item.getId(), itemFields(item));
    }
    app.freerouting.core.scoring.BoardStatistics statsBefore =
        new app.freerouting.core.scoring.BoardStatistics(board);
    java.util.SortedSet<Item> ripped = new java.util.TreeSet<>();
    java.util.Map<Item, Integer> costs = new java.util.LinkedHashMap<>();
    // AutorouteConnectionRouter.route at ripup pass 1: the maze search's WorkLimit.
    int routeNet = conn.net;
    app.freerouting.datastructures.WorkLimit limit = new app.freerouting.datastructures.WorkLimit(
        100000L, "maze_search", () -> "net=" + routeNet);
    long start = workUnits();
    AutorouteEngine engine = board.initAutoroute(conn.net, c.traceClearanceClassIndex, null,
        limit, false);
    app.freerouting.autoroute.AutorouteAttemptResult result;
    try {
      result = engine.autorouteConnection(conn.start, conn.dest, c, ripped, costs);
    } catch (Exception e) {
      out.line(new Line().str("exception", e.getClass().getSimpleName()).end());
      return;
    }
    long units = workUnits() - start;
    out.line(new Line().str("state", result.state.name())
        .str("details", result.details == null ? "" : result.details).end());
    out.line(new Line().raw("ripped", ids(ripped)).raw("costs", costsJson(costs)).end());
    java.util.TreeMap<Integer, String> after = new java.util.TreeMap<>();
    for (Item item : board.getItems()) {
      after.put(item.getId(), itemFields(item));
    }
    List<Integer> removed = new ArrayList<>();
    for (var entry : before.entrySet()) {
      String now = after.get(entry.getKey());
      if (now == null) {
        removed.add(entry.getKey());
      } else if (!now.equals(entry.getValue())) {
        out.line("{\"changed\":" + entry.getKey() + "," + now + "}");
      }
    }
    for (var entry : after.entrySet()) {
      if (!before.containsKey(entry.getKey())) {
        out.line("{\"added\":" + entry.getKey() + "," + entry.getValue() + "}");
      }
    }
    out.line(new Line().raw("removed", ints(removed)).end());
    app.freerouting.core.scoring.BoardStatistics statsAfter =
        new app.freerouting.core.scoring.BoardStatistics(board);
    out.line(new Line().num("maxGeneratedId", board.communication.idGenerator.maxGeneratedId())
        .num("incomplete", statsAfter.connections.incompleteCount
            - statsBefore.connections.incompleteCount)
        .num("vias", statsAfter.vias.totalCount - statsBefore.vias.totalCount)
        .dbl("lengthBefore", statsBefore.traces.totalLength)
        .dbl("lengthAfter", statsAfter.traces.totalLength)
        .num("units", units).end());
  }

  /** insert-routed: insertConnection through pre-routed traces, as maze-routed walks them. */
  static void insertRouted(Out out, Loaded l) throws Exception {
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
      insertConnection(out, l, conn);
    }
  }

  /** insert: every sampled connection, each on a fresh load of the board. */
  static void insert(Out out, String dsn) throws Exception {
    Loaded first = Loaded.load(dsn);
    List<Conn> conns = sample(first.board);
    sampleLine(out, first.board, conns);
    for (int i = 0; i < conns.size(); i++) {
      Loaded l = i == 0 ? first : Loaded.load(dsn);
      Conn conn = sample(l.board).get(i);
      insertConnection(out, l, conn);
    }
  }

  // ------------------------------------------------------------------------------------------
  // pass (A11.7)
  // ------------------------------------------------------------------------------------------

  /** A stoppable thread nobody stops (the job's thread for the probe's BatchAutorouter). */
  static app.freerouting.core.StoppableThread idleThread() {
    return new app.freerouting.core.StoppableThread() {
      @Override
      protected void threadAction() {}
    };
  }

  /** A declared method of {@code type}, accessible. */
  static java.lang.reflect.Method method(Class<?> type, String name, Class<?>... params)
      throws Exception {
    java.lang.reflect.Method m = type.getDeclaredMethod(name, params);
    m.setAccessible(true);
    return m;
  }

  /** Sets a declared field of {@code target}'s class (or a superclass) by reflection. */
  static void setField(Object target, String name, Object value) throws Exception {
    Class<?> type = target.getClass();
    while (type != null) {
      try {
        java.lang.reflect.Field field = type.getDeclaredField(name);
        field.setAccessible(true);
        field.set(target, value);
        return;
      } catch (NoSuchFieldException e) {
        type = type.getSuperclass();
      }
    }
    throw new NoSuchFieldException(name);
  }

  /** FNV-1a 64 of every trace and via, "{"item":id,fields}" in id order. */
  static long routeHash(RoutingBoard board) {
    java.util.TreeMap<Integer, String> lines = new java.util.TreeMap<>();
    for (Item item : board.getItems()) {
      if (isRouteItem(item)) {
        lines.put(item.getId(), "{\"item\":" + item.getId() + "," + itemFields(item) + "}");
      }
    }
    return fnvLines(lines.values());
  }

  /** The traces and vias with an id above {@code maxId}, "{"added":id,fields}" in id order. */
  static List<String> addedLines(RoutingBoard board, int maxId) {
    java.util.TreeMap<Integer, String> lines = new java.util.TreeMap<>();
    for (Item item : board.getItems()) {
      if (isRouteItem(item) && item.getId() > maxId) {
        lines.put(item.getId(), "{\"added\":" + item.getId() + "," + itemFields(item) + "}");
      }
    }
    return new ArrayList<>(lines.values());
  }

  static BatchRouter batchRouter(Loaded l) throws Exception {
    l.job.thread = idleThread();
    return new BatchRouter(new app.freerouting.autoroute.pipeline.BatchAutorouter(l.job));
  }

  /** The probe's handle on a BatchAutorouter's package-private and private members. */
  static final class BatchRouter {
    final Object router;
    final Class<?> type;

    BatchRouter(Object router) {
      this.router = router;
      this.type = router.getClass();
    }

    RoutingBoard board() throws Exception {
      return privateField(router, "board");
    }

    Object passRunner() throws Exception {
      return privateField(router, "passRunner");
    }
  }

  /** One replicated pass; returns whether it routed or failed something (Java's result). */
  @SuppressWarnings("unchecked")
  static boolean replicaPass(Out out, BatchRouter r, int passNo, long[] hashes) throws Exception {
    RoutingBoard board = r.board();
    List<Item> items = (List<Item>) method(r.type, "getAutorouteItems", RoutingBoard.class)
        .invoke(r.router, board);
    if (items.isEmpty()) {
      out.line(new Line().str("stage", "pass:" + passNo).raw("items", "[]").end());
      hashes[passNo - 1] = routeHash(board);
      return false;
    }
    Object runner = r.passRunner();
    if (passNo > 1) {
      method(runner.getClass(), "reorderSingleThreadItems", List.class, int.class)
          .invoke(runner, items, passNo);
    }
    out.line(new Line().str("stage", "pass:" + passNo).raw("items", ids(items)).end());
    java.lang.reflect.Method autorouteItem = method(r.type, "autorouteItem", Item.class,
        int.class, java.util.SortedSet.class, java.util.Map.class, int.class);
    int routed = 0;
    int notRouted = 0;
    int index = 0;
    for (Item currentItem : items) {
      for (int i = 0; i < currentItem.netCount(); i++) {
        int totalItemsRouted = privateField(r.router, "totalItemsRouted");
        setField(r.router, "totalItemsRouted", totalItemsRouted + 1);
        board = r.board();
        board.startMarkingChangedArea();
        java.util.SortedSet<Item> ripped = new java.util.TreeSet<>();
        java.util.Map<Item, Integer> costs = new java.util.LinkedHashMap<>();
        int net = currentItem.getNetNumber(i);
        int maxBefore = board.communication.idGenerator.maxGeneratedId();
        long start = workUnits();
        app.freerouting.autoroute.AutorouteAttemptResult result =
            (app.freerouting.autoroute.AutorouteAttemptResult) autorouteItem.invoke(
                r.router, currentItem, net, ripped, costs, passNo);
        board = r.board();
        // AutoroutePassRunner.java:150-198.
        if (result.state == app.freerouting.autoroute.AutorouteAttemptState.ROUTED) {
          ++routed;
        } else if (result.state == app.freerouting.autoroute.AutorouteAttemptState.ALREADY_CONNECTED
            || result.state == app.freerouting.autoroute.AutorouteAttemptState.NO_UNCONNECTED_NETS
            || result.state
                == app.freerouting.autoroute.AutorouteAttemptState.CONNECTED_TO_PLANE) {
          // skipped
        } else {
          board.failureLog.recordFailure(currentItem, passNo, result.state, result.details);
          int failureCount = board.failureLog.getFailureCount(currentItem);
          if (failureCount >= 2) {
            List<Item> tracesToRip = new ArrayList<>();
            for (Item netItem : board.getConnectableItems(net)) {
              if (isRouteItem(netItem) && !netItem.isUserFixed() && netItem.netCount() == 1) {
                tracesToRip.add(netItem);
              }
            }
            if (!tracesToRip.isEmpty()) {
              board.removeItems(tracesToRip);
            }
          }
          ++notRouted;
        }
        long units = workUnits() - start;
        List<String> added = addedLines(board, maxBefore);
        out.line(new Line().num("conn", index).num("item", currentItem.getId()).num("net", net)
            .str("state", result.state.name()).raw("ripped", ids(ripped))
            .num("maxIdBefore", maxBefore)
            .num("maxIdAfter", board.communication.idGenerator.maxGeneratedId())
            .num("added", added.size()).str("addedHash", hex(fnvLines(added)))
            .str("boardHash", hex(routeHash(board)))
            .num("failures", board.failureLog.getFailureCount(currentItem))
            .num("units", units).end());
        index++;
      }
    }
    // BatchAutorouter.removeTails, as runSingleThread calls it.
    board = r.board();
    boolean removeUnconnectedVias =
        (boolean) method(r.type, "isRemoveUnconnectedVias").invoke(r.router);
    app.freerouting.board.model.items.Item.StopConnectionOption option = removeUnconnectedVias
        ? app.freerouting.board.model.items.Item.StopConnectionOption.NONE
        : app.freerouting.board.model.items.Item.StopConnectionOption.FANOUT_VIA;
    int accuracy = (int) method(r.type, "getTracePullTightAccuracy").invoke(r.router);
    AutorouteControl.ExpansionCostFactor[] traceCosts =
        (AutorouteControl.ExpansionCostFactor[]) method(r.type, "getTraceCosts").invoke(r.router);
    app.freerouting.core.StoppableThread thread = privateField(r.router, "thread");
    long start = workUnits();
    board.startMarkingChangedArea();
    boolean removed = board.removeTraceTails(-1, option);
    board.optChangedArea(new int[0], null, accuracy, traceCosts, thread, 1000);
    long units = workUnits() - start;
    int incomplete = board.routingLedger().incompleteCount();
    java.util.Set<Integer> nets = new java.util.TreeSet<>(board.routingLedger()
        .incompleteNetNumbers());
    // AutoroutePassRunner.java:233-239, the anti-oscillation state.
    int previous = privateField(runner, "previousIncompleteCount");
    int stagnation = privateField(runner, "stagnationCount");
    setField(runner, "stagnationCount",
        previous >= 0 && incomplete >= previous ? stagnation + 1 : 0);
    setField(runner, "previousIncompleteCount", incomplete);
    setField(runner, "previousIncompleteNets", nets);
    boolean result = routed > 0 || notRouted > 0;
    long hash = routeHash(board);
    hashes[passNo - 1] = hash;
    out.line(new Line().num("tails", passNo).bool("removed", removed).num("units", units)
        .num("incomplete", incomplete).raw("incompleteNets", ints(nets))
        .bool("continue", result)
        .num("maxGeneratedId", board.communication.idGenerator.maxGeneratedId())
        .str("boardHash", hex(hash)).end());
    return result;
  }

  /** pass: the replica's passes 1 and 2, then the real ones on a second load. */
  static void pass(Out out, String dsn) throws Exception {
    long[] replica = new long[2];
    BatchRouter r = batchRouter(Loaded.load(dsn));
    for (int passNo = 1; passNo <= 2; passNo++) {
      replicaPass(out, r, passNo, replica);
    }
    BatchRouter real = batchRouter(Loaded.load(dsn));
    java.lang.reflect.Method autoroutePass = method(real.type, "autoroutePass", int.class);
    boolean[] same = new boolean[2];
    for (int passNo = 1; passNo <= 2; passNo++) {
      autoroutePass.invoke(real.router, passNo);
      same[passNo - 1] = routeHash(real.board()) == replica[passNo - 1];
    }
    out.line(new Line().str("stage", "real").bool("pass1", same[0]).bool("pass2", same[1])
        .end());
  }

  // ------------------------------------------------------------------------------------------
  // fanout (A12.6)
  // ------------------------------------------------------------------------------------------

  /** The skip reason of BatchFanout.fanoutPass's via check (:268-289), or null. */
  static String fanoutViaSkip(RoutingBoard board, RouterSettings settings, int netNumber) {
    Net net = board.rules.nets.get(netNumber);
    if (net == null) {
      return null;
    }
    app.freerouting.rules.NetClass netClass = net.getNetClass();
    app.freerouting.rules.ViaRule viaRule = netClass != null ? netClass.getViaRule() : null;
    boolean hasBoardVias =
        !board.rules.viaRules.isEmpty() && board.rules.viaRules.firstElement().viaCount() > 0;
    boolean fallbackAllowed = settings.fanout != null
        && Boolean.TRUE.equals(settings.fanout.fallbackToBoardVias) && hasBoardVias;
    boolean canUseVias = (viaRule != null && viaRule.viaCount() > 0) || fallbackAllowed;
    return canUseVias ? null : "no_vias";
  }

  /** The replica of fanoutPass(0) on l's board; returns its route hash. */
  @SuppressWarnings("unchecked")
  static long replicaFanoutPass(Out out, Loaded l) throws Exception {
    RoutingBoard board = l.board;
    RouterSettings settings = l.job.routerSettings;
    app.freerouting.core.StoppableThread thread = l.job.thread;
    java.lang.reflect.Constructor<?> ctor =
        app.freerouting.autoroute.pipeline.BatchFanout.class.getDeclaredConstructor(
            RoutingBoard.class, RouterSettings.class, app.freerouting.core.StoppableThread.class);
    ctor.setAccessible(true);
    Object fanout = ctor.newInstance(board, settings, thread);
    java.util.SortedSet<Object> components = privateField(fanout, "sortedComponents");
    int passNo = 0;
    int ripupCosts = settings.getStartRipupCosts() * (passNo + 1);
    long baseMillisPerPin = settings.fanout != null && settings.fanout.maxMillisecondsPerPin != null
        ? settings.fanout.maxMillisecondsPerPin
        : 10000L;
    boolean ripupAllowed = (settings.fanout == null || settings.fanout.ripupAllowed == null)
        || Boolean.TRUE.equals(settings.fanout.ripupAllowed);
    int effectiveRipupCosts = ripupAllowed ? ripupCosts : -1;
    java.util.Map<Integer, Integer> failedPinGeneration = new java.util.HashMap<>();
    java.util.Map<Integer, Integer> componentGeneration = new java.util.HashMap<>();
    int totalItemsFanouted = 0;
    int routed = 0;
    int notRouted = 0;
    int insertErrors = 0;
    int alreadyConnected = 0;
    int viasBefore = board.getVias().size();
    int index = 0;
    outer:
    for (Object component : components) {
      app.freerouting.board.model.structure.Component boardComponent =
          privateField(component, "boardComponent");
      java.util.SortedSet<Object> pins = privateField(component, "smdPins");
      out.line(new Line().str("stage", "component:" + boardComponent.id)
          .num("pins", pins.size()).end());
      for (Object fanoutPin : pins) {
        app.freerouting.board.model.items.Pin pin = privateField(fanoutPin, "boardPin");
        if (settings.fanout != null && settings.fanout.maxItems != null
            && settings.fanout.maxItems > 0 && totalItemsFanouted >= settings.fanout.maxItems) {
          break outer;
        }
        long start = workUnits();
        int maxBefore = board.communication.idGenerator.maxGeneratedId();
        double maxMilliseconds = baseMillisPerPin * (passNo + 1);
        app.freerouting.datastructures.WorkLimit workLimit =
            new app.freerouting.datastructures.WorkLimit((long) maxMilliseconds, "fanout_pin",
                () -> "");
        String state;
        boolean strict = false;
        int netNumber = pin.getNetNumber(0);
        if (pin.hasIgnoredNets()) {
          state = "skip:ignored_net";
        } else {
          // BatchFanout.java:264, targetCount: a log value, but its queries count work.
          pin.getUnconnectedSet(netNumber).size();
          String skip = fanoutViaSkip(board, settings, netNumber);
          Integer failed = failedPinGeneration.get(pin.getId());
          int generation = componentGeneration.getOrDefault(boardComponent.id, 0);
          if (skip != null) {
            state = "skip:" + skip;
          } else if (failed != null && failed.intValue() == generation) {
            state = "skip:no_retry";
          } else {
            maxBefore = board.communication.idGenerator.maxGeneratedId();
            board.startMarkingChangedArea();
            app.freerouting.autoroute.AutorouteAttemptResult result =
                board.fanout(pin, settings, effectiveRipupCosts, thread, workLimit);
            if (result.state == app.freerouting.autoroute.AutorouteAttemptState.ROUTED) {
              app.freerouting.autoroute.AutorouteAttemptResult rejection =
                  app.freerouting.autoroute.pipeline.BatchAutorouter.enforceStrictDrc(
                      board, netNumber, maxBefore);
              if (rejection != null) {
                result = rejection;
                strict = true;
              }
            }
            switch (result.state) {
              case ROUTED -> {
                ++routed;
                totalItemsFanouted++;
                componentGeneration.merge(boardComponent.id, 1, Integer::sum);
              }
              case ALREADY_CONNECTED -> ++alreadyConnected;
              case FAILED, INSERT_ERROR -> {
                if (result.state == app.freerouting.autoroute.AutorouteAttemptState.FAILED) {
                  ++notRouted;
                } else {
                  ++insertErrors;
                }
                totalItemsFanouted++;
                failedPinGeneration.put(pin.getId(),
                    componentGeneration.getOrDefault(boardComponent.id, 0));
              }
              default -> {}
            }
            state = result.state.name();
          }
        }
        long units = workUnits() - start;
        List<String> added = addedLines(board, maxBefore);
        out.line(new Line().num("pin", index).num("component", boardComponent.id)
            .num("item", pin.getId()).num("net", netNumber).str("state", state)
            .bool("strict", strict).num("maxIdBefore", maxBefore)
            .num("maxIdAfter", board.communication.idGenerator.maxGeneratedId())
            .num("added", added.size()).str("addedHash", hex(fnvLines(added)))
            .str("boardHash", hex(routeHash(board))).num("units", units).end());
        index++;
      }
    }
    long hash = routeHash(board);
    out.line(new Line().num("pass", passNo).num("routed", routed).num("notRouted", notRouted)
        .num("insertErrors", insertErrors).num("alreadyConnected", alreadyConnected)
        .num("extraVias", Math.max(0, board.getVias().size() - viasBefore))
        .num("maxGeneratedId", board.communication.idGenerator.maxGeneratedId())
        .str("boardHash", hex(hash)).end());
    return hash;
  }

  /**
   * fanout: the replica's pass 0, then the real one-pass fanoutBoard on a second load. With
   * {@code maxMillisecondsPerPin} non-null, both use that per-pin budget.
   */
  static void fanout(Out out, String dsn, Long maxMillisecondsPerPin, String escapesOf)
      throws Exception {
    Loaded replica = Loaded.load(dsn);
    replica.job.thread = idleThread();
    Loaded real = Loaded.load(dsn);
    real.job.thread = idleThread();
    for (Loaded l : List.of(replica, real)) {
      l.job.routerSettings.fanout.maxPasses = 1;
      if (maxMillisecondsPerPin != null) {
        l.job.routerSettings.fanout.maxMillisecondsPerPin = maxMillisecondsPerPin;
      }
    }
    long replicaHash = replicaFanoutPass(out, replica);
    String replicaBoardHash = replica.board.getHash();
    app.freerouting.autoroute.pipeline.BatchFanout.fanoutBoard(
        real.board, real.job.routerSettings, real.job.thread);
    boolean same = routeHash(real.board) == replicaHash
        && real.board.getHash().equals(replicaBoardHash);
    out.line(new Line().str("stage", "real").bool("replica_matches_real", same).end());
    if (escapesOf != null) {
      // The escape count (BoardStatistics.isPinEscaped) of the component's SMD pins with a net.
      app.freerouting.board.model.structure.Component component =
          replica.board.components.get(escapesOf);
      int pins = 0;
      int escaped = 0;
      for (app.freerouting.board.model.items.Pin pin : replica.board.getSmdPins()) {
        if (pin.getComponentId() == component.id && pin.netCount() > 0) {
          pins++;
          if (app.freerouting.core.scoring.BoardStatistics.isPinEscaped(pin)) {
            escaped++;
          }
        }
      }
      out.line(new Line().str("stage", "escapes").str("component", escapesOf)
          .num("smdPins", pins).num("escaped", escaped).end());
    }
  }

  /** plane-island: fanout, then which planes each plane-net SMD pin reaches. */
  static void planeIsland(Out out, String dsn) throws Exception {
    Loaded l = Loaded.load(dsn);
    l.job.thread = idleThread();
    app.freerouting.autoroute.pipeline.BatchFanout.fanoutBoard(
        l.board, l.job.routerSettings, l.job.thread);
    out.line(new Line().str("stage", "pin-planes").str("boardHash", hex(routeHash(l.board)))
        .end());
    for (app.freerouting.board.model.items.Pin pin : l.board.getPins()) {
      if (pin.firstLayer() != pin.lastLayer() || pin.netCount() != 1) {
        continue;
      }
      int net = pin.getNetNumber(0);
      Net rulesNet = l.board.rules.nets.get(net);
      if (rulesNet == null || !rulesNet.containsPlane()) {
        continue;
      }
      java.util.TreeSet<Integer> areas = new java.util.TreeSet<>();
      for (Item item : pin.getConnectedSet(net)) {
        if (item instanceof ConductionArea) {
          areas.add(item.getId());
        }
      }
      out.line(new Line().num("pinPlanes", pin.getId()).num("net", net).raw("areas", ints(areas))
          .end());
    }
    app.freerouting.core.scoring.BoardStatistics stats =
        new app.freerouting.core.scoring.BoardStatistics(l.board, null, false);
    out.line(new Line().num("incomplete", stats.connections.incompleteCount).end());
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
  // last-mile (A13.3)
  // ------------------------------------------------------------------------------------------

  /** The board of last-mile: the load, the routed reference SES imported, traces and vias unfixed. */
  static Loaded lastMileBoard(String dsn) throws Exception {
    Loaded l = Loaded.load(dsn);
    Path ses = Path.of("fixtures/reference/route", board(dsn) + ".ses");
    try (var in = Files.newInputStream(ses)) {
      app.freerouting.io.specctra.SesReader.read(in, l.board);
    }
    for (Item item : l.board.getItems()) {
      if (isRouteItem(item)) {
        item.unfix();
      }
    }
    return l;
  }

  static void lastMile(Out out, String dsn) throws Exception {
    Class<?> ripup = Class.forName("app.freerouting.autoroute.pipeline.LastMileBlockerRipup");
    java.lang.reflect.Method corridor =
        ripup.getDeclaredMethod("corridor", FloatPoint.class, FloatPoint.class);
    corridor.setAccessible(true);
    java.lang.reflect.Method removable =
        ripup.getDeclaredMethod("isRemovableBlocker", Item.class, int.class);
    removable.setAccessible(true);
    java.lang.reflect.Field maxField = ripup.getDeclaredField("MAX_ITEMS");
    maxField.setAccessible(true);
    int maxItems = maxField.getInt(null);
    java.lang.reflect.Method rip = ripup.getDeclaredMethod("ripBlockers", RoutingBoard.class);
    rip.setAccessible(true);

    RoutingBoard board = lastMileBoard(dsn).board;
    app.freerouting.drc.AirLine[] airlines =
        new app.freerouting.drc.DesignRulesChecker(board, null).getAllAirlines();
    out.line(new Line().str("stage", "airlines").num("count", airlines.length)
        .num("maxItems", maxItems).end());
    java.util.LinkedHashSet<Item> blockers = new java.util.LinkedHashSet<>();
    for (int i = 0; i < airlines.length; i++) {
      app.freerouting.drc.AirLine airline = airlines[i];
      if (airline == null || airline.net == null) {
        out.line(new Line().num("airline", i).bool("net", false).end());
        continue;
      }
      IntBox box = (IntBox) corridor.invoke(null, airline.fromCorner, airline.toCorner);
      List<Integer> overlapping = new ArrayList<>();
      List<Integer> added = new ArrayList<>();
      for (app.freerouting.board.searchtree.SearchTreeObject object :
          board.overlappingObjects(box, -1)) {
        if (!(object instanceof Item item)) {
          continue;
        }
        overlapping.add(item.getId());
        if (blockers.size() >= maxItems) {
          break;
        }
        if ((Boolean) removable.invoke(null, item, airline.net.netNumber) && blockers.add(item)) {
          added.add(item.getId());
        }
      }
      out.line(new Line().num("airline", i).num("net", airline.net.netNumber)
          .num("from", airline.fromItem.getId()).num("to", airline.toItem.getId())
          .dbl("fromX", airline.fromCorner.x).dbl("fromY", airline.fromCorner.y)
          .dbl("toX", airline.toCorner.x).dbl("toY", airline.toCorner.y)
          .raw("corridor", intArray(new int[] {box.ll.x, box.ll.y, box.ur.x, box.ur.y}))
          .raw("overlapping", ints(overlapping)).raw("added", ints(added)).end());
      if (blockers.size() >= maxItems) {
        break;
      }
    }
    List<Integer> blockerIds = new ArrayList<>();
    for (Item item : blockers) {
      blockerIds.add(item.getId());
    }
    out.line(new Line().str("stage", "blockers").raw("ids", ints(blockerIds)).end());
    board.removeItems(new ArrayList<>(blockers));
    RoutingBoard real = lastMileBoard(dsn).board;
    int removed = (Integer) rip.invoke(null, real);
    out.line(new Line().str("stage", "real").num("removed", removed)
        .bool("replica_matches_real", itemIds(board).equals(itemIds(real))).end());
  }

  static List<Integer> itemIds(RoutingBoard board) {
    List<Integer> ids = new ArrayList<>();
    for (Item item : board.getItems()) {
      ids.add(item.getId());
    }
    return ids;
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
