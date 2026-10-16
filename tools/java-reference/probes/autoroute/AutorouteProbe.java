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
