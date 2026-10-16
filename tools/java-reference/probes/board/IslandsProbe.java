import app.freerouting.Freerouting;
import app.freerouting.board.facade.RoutingBoard;
import app.freerouting.board.model.items.ConductionArea;
import app.freerouting.board.model.items.Connectable;
import app.freerouting.board.model.items.DrillItem;
import app.freerouting.board.model.items.Item;
import app.freerouting.board.model.items.Trace;
import app.freerouting.board.model.structure.Unit;
import app.freerouting.core.RoutingJob;
import app.freerouting.drc.DesignRulesChecker;
import app.freerouting.drc.ZoneIslandViolation;
import app.freerouting.geometry.planar.FloatPoint;
import app.freerouting.geometry.planar.Point;
import app.freerouting.io.specctra.SesReader;
import app.freerouting.management.BoardLoader;
import app.freerouting.settings.GlobalSettings;
import java.awt.geom.Area;
import java.awt.geom.Path2D;
import java.awt.geom.PathIterator;
import java.awt.geom.Rectangle2D;
import java.io.FileInputStream;
import java.io.InputStream;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.Collection;
import java.util.Comparator;
import java.util.HashMap;
import java.util.List;
import java.util.Map;
import java.util.UUID;

/**
 * Board probe {@code islands} (B07.3): Java's copper islands. Loads the board, imports the
 * session and, per conduction area in id order, writes
 *
 * <ul>
 *   <li>an {@code area} line: id, layer, net, filled, the number of islands of Java's
 *       decomposition of {@code getDetailedFillArea()} ({@code
 *       DesignRulesChecker.decomposeAreaIntoIslands}, replayed by a copy that also counts the
 *       merges), {@code primary} (the index of the island Java judges connectivity against, the
 *       first with the most items; -1 for fewer than two islands) and {@code missing}, the area
 *       of the fill that no island covers;
 *   <li>one {@code island} line per island, in Java's order: bounds ({@code getBounds2D}), {@code
 *       awtArea} (the shoelace area of the flattened path), {@code extra} (the area of the island
 *       outside the fill), {@code xorMerges} (subpaths merged into the island by {@code
 *       exclusiveOr} because they were not inside it) and {@code holeOutside}: true when such a
 *       subpath exists or the island reaches outside the fill, i.e. the island holds a hole
 *       contour that lies outside its own outer boundary, or is one (Java's hole pairing bug,
 *       docs/DIVERGENCES.md "Design rule check").
 * </ul>
 *
 * Then one {@code finding} line per {@code getZoneIslandViolations()} result, in Java's order:
 * area id, type, layer, net, the island index ({@code -1} if no island of the copy equals it, so
 * an index shows that the copy agrees with Java's decomposition), its bounds,
 * {@code awtArea} and the sorted ids of its items.
 *
 * <p>Last, the same check with the hole pairing fixed (the port's behaviour, docs/DIVERGENCES.md):
 * each subpath of the fill is an outer contour (negative signed area: AWT orients copper
 * clockwise in board coordinates) or a hole (positive), a hole belongs to the smallest outer
 * contour that contains it, and an island is an outer contour minus its holes, ordered by the
 * lowest y, then the lowest x of its bounds, as the port orders them. Per conduction area one
 * {@code fixedIsland} line per island (bounds, {@code awtArea}, a point well inside it), then one
 * {@code fixed} line per
 * finding of {@code getZoneIslandViolations}' loop run on these islands (a copy of it, below).
 * Exits 3 on boards without conduction areas.
 *
 * <pre>
 * tools/java-reference/probe.sh &lt;pin&gt; tools/java-reference/probes/board/IslandsProbe.java \
 *     board.dsn board.ses out.jsonl
 * </pre>
 */
public class IslandsProbe {
  /** The flatness of the path iterator for the shoelace areas, in board units. */
  static final double FLATNESS = 1.0;

  public static void main(String[] args) throws Exception {
    Boards.Out out = new Boards.Out("islands", args[0]);
    try {
      RoutingBoard board = Boards.load(args[0]);
      if (board.getConductionAreas().isEmpty()) {
        System.exit(3);
      }
      if (!args[1].equals("-")) {
        Boards.importSession(board, args[1]);
      }
      DesignRulesChecker checker = new DesignRulesChecker(board, null);
      List<ConductionArea> areas = new ArrayList<>(board.getConductionAreas());
      areas.sort(Comparator.comparingInt(Item::getId));
      Map<Integer, List<Area>> islandsById = new HashMap<>();
      for (ConductionArea ca : areas) {
        islandsById.put(ca.getId(), writeArea(out, board, ca));
      }
      List<ZoneIslandViolation> findings = new ArrayList<>(checker.getZoneIslandViolations());
      for (int i = 0; i < findings.size(); i++) {
        ZoneIslandViolation v = findings.get(i);
        writeFinding(out, islandsById.get(v.conductionArea.getId()), i, v);
      }
      int fixedIndex = 0;
      for (ConductionArea ca : areas) {
        fixedIndex = writeFixed(out, board, ca, fixedIndex);
      }
    } catch (Throwable t) {
      out.exception(t);
    }
    out.write(args[2]);
  }

  /** Writes the area line and the island lines of {@code ca}; returns its islands. */
  static List<Area> writeArea(Boards.Out out, RoutingBoard board, ConductionArea ca) {
    Area fill = ca.getDetailedFillArea();
    List<Integer> xors = new ArrayList<>();
    List<Area> islands = replayIslands(fill, xors);
    int primary = islands.size() > 1 ? primaryIndex(islandItems(islands, sameNetItems(board, ca))) : -1;
    Area covered = new Area();
    for (Area island : islands) {
      covered.add(island);
    }
    Area missing = fill == null ? new Area() : new Area(fill);
    missing.subtract(covered);
    out.line(
        new Boards.Line()
            .num("area", ca.getId())
            .num("layer", ca.getLayer())
            .num("net", ca.netCount() > 0 ? ca.getNetNumber(0) : 0)
            .bool("filled", ca.getIsFilled())
            .num("islands", islands.size())
            .num("primary", primary)
            .dbl("missing", shoelace(missing))
            .end());
    for (int i = 0; i < islands.size(); i++) {
      Area island = islands.get(i);
      Area extra = new Area(island);
      extra.subtract(fill);
      int xorMerges = xors.get(i);
      Boards.Line line = new Boards.Line().num("area", ca.getId()).num("island", i);
      bounds(line, island)
          .dbl("awtArea", shoelace(island))
          .dbl("extra", shoelace(extra))
          .num("xorMerges", xorMerges)
          .bool("holeOutside", xorMerges > 0 || !extra.isEmpty());
      out.line(line.end());
    }
    return islands;
  }

  static void writeFinding(Boards.Out out, List<Area> islands, int index, ZoneIslandViolation v) {
    int island = -1;
    for (int i = 0; i < islands.size(); i++) {
      if (islands.get(i).getBounds2D().equals(v.islandArea.getBounds2D())
          && islands.get(i).equals(v.islandArea)) {
        island = i;
        break;
      }
    }
    Boards.Line line =
        new Boards.Line()
            .num("finding", index)
            .num("area", v.conductionArea.getId())
            .str("type", v.type)
            .num("layer", v.layer)
            .num("net", v.netNumber)
            .num("island", island);
    bounds(line, v.islandArea)
        .dbl("awtArea", shoelace(v.islandArea))
        .raw("items", ids(v.itemsInIsland));
    out.line(line.end());
  }

  /** The fixed decomposition of {@code ca}'s fill and the findings on it; see the class doc. */
  static int writeFixed(Boards.Out out, RoutingBoard board, ConductionArea ca, int index) {
    List<Area> islands = fixedIslands(ca.getDetailedFillArea());
    for (int i = 0; i < islands.size(); i++) {
      Boards.Line line = new Boards.Line().num("area", ca.getId()).num("fixedIsland", i);
      double[] inside = insidePoint(islands.get(i));
      out.line(
          bounds(line, islands.get(i))
              .dbl("awtArea", shoelace(islands.get(i)))
              .dbl("insideX", inside[0])
              .dbl("insideY", inside[1])
              .end());
    }
    if (islands.size() <= 1) {
      return index;
    }
    double minDeadCopperArea =
        1.0 * board.communication.getResolution(Unit.MM) * board.communication.getResolution(Unit.MM);
    int netNumber = ca.netCount() > 0 ? ca.getNetNumber(0) : 0;
    // From here a copy of the loop of DesignRulesChecker.getZoneIslandViolations at the pin.
    List<List<Item>> islandItems = islandItems(islands, sameNetItems(board, ca));
    int primaryIndex = primaryIndex(islandItems);
    for (int i = 0; i < islands.size(); i++) {
      Area island = islands.get(i);
      List<Item> items = islandItems.get(i);
      String type = null;
      if (!items.isEmpty()) {
        if (i != primaryIndex) {
          boolean connectedToPrimary = false;
          if (primaryIndex >= 0 && !islandItems.get(primaryIndex).isEmpty()) {
            Item repItemInPrimary = islandItems.get(primaryIndex).get(0);
            Collection<Item> connectedSet = repItemInPrimary.getConnectedSet(netNumber, true);
            for (Item item : items) {
              if (connectedSet.contains(item)) {
                connectedToPrimary = true;
                break;
              }
            }
          }
          if (!connectedToPrimary) {
            type = "isolated_island_unconnected";
          }
        }
      } else {
        Rectangle2D bounds = island.getBounds2D();
        double approxArea = bounds.getWidth() * bounds.getHeight();
        if (approxArea >= minDeadCopperArea) {
          type = "isolated_island_dead_copper";
        }
      }
      if (type == null) {
        continue;
      }
      Boards.Line line =
          new Boards.Line()
              .num("fixed", index++)
              .num("area", ca.getId())
              .str("type", type)
              .num("layer", ca.getLayer())
              .num("net", netNumber)
              .num("island", i);
      bounds(line, island).dbl("awtArea", shoelace(island)).raw("items", ids(items));
      out.line(line.end());
    }
    return index;
  }

  /**
   * A point well inside {@code island}: the first point of a 17 x 17 grid over its bounds that is
   * the centre of a square of half side h inside the island, for h from a quarter of the smaller
   * side of the bounds down to 1/64 of it; else the first grid point inside; else the centre.
   */
  static double[] insidePoint(Area island) {
    Rectangle2D b = island.getBounds2D();
    double side = Math.min(b.getWidth(), b.getHeight());
    for (double h = side / 4.0; h >= side / 64.0; h /= 2.0) {
      for (int i = 1; i < 18; i++) {
        for (int j = 1; j < 18; j++) {
          double x = b.getMinX() + b.getWidth() * j / 18.0;
          double y = b.getMinY() + b.getHeight() * i / 18.0;
          if (island.contains(new Rectangle2D.Double(x - h, y - h, 2 * h, 2 * h))) {
            return new double[] {x, y};
          }
        }
      }
    }
    for (int i = 1; i < 18; i++) {
      for (int j = 1; j < 18; j++) {
        double x = b.getMinX() + b.getWidth() * j / 18.0;
        double y = b.getMinY() + b.getHeight() * i / 18.0;
        if (island.contains(x, y)) {
          return new double[] {x, y};
        }
      }
    }
    return new double[] {b.getCenterX(), b.getCenterY()};
  }

  /** The connectable items of {@code ca}'s net on its layer, as Java collects them. */
  static List<Item> sameNetItems(RoutingBoard board, ConductionArea ca) {
    List<Item> sameNetItemsOnLayer = new ArrayList<>();
    for (Item item : board.getItems()) {
      if (item != ca && item instanceof Connectable && item.sharesNet(ca) && item.sharesLayer(ca)) {
        sameNetItemsOnLayer.add(item);
      }
    }
    return sameNetItemsOnLayer;
  }

  /** The items of each island: those whose centre (first corner for traces) lies on it. */
  static List<List<Item>> islandItems(List<Area> islands, List<Item> sameNetItemsOnLayer) {
    List<List<Item>> islandItems = new ArrayList<>(islands.size());
    for (Area island : islands) {
      List<Item> contained = new ArrayList<>();
      for (Item item : sameNetItemsOnLayer) {
        Point center = null;
        if (item instanceof DrillItem drillItem) {
          center = drillItem.getCenter();
        } else if (item instanceof Trace trace) {
          center = trace.firstCorner();
        }
        if (center != null) {
          FloatPoint fp = center.toFloat();
          if (island.contains(fp.x, fp.y)) {
            contained.add(item);
          }
        }
      }
      islandItems.add(contained);
    }
    return islandItems;
  }

  /** The primary island: the first with the most items. */
  static int primaryIndex(List<List<Item>> islandItems) {
    int primaryIndex = -1;
    int maxConnectedItems = -1;
    for (int i = 0; i < islandItems.size(); i++) {
      int count = islandItems.get(i).size();
      if (count > maxConnectedItems) {
        maxConnectedItems = count;
        primaryIndex = i;
      }
    }
    return primaryIndex;
  }

  /** The islands of {@code fill} with every hole in the smallest outer contour around it. */
  static List<Area> fixedIslands(Area fill) {
    List<Area> outers = new ArrayList<>();
    List<Area> holes = new ArrayList<>();
    if (fill == null || fill.isEmpty()) {
      return outers;
    }
    for (Path2D.Double path : subpaths(fill)) {
      Area piece = new Area(path);
      if (piece.isEmpty()) {
        continue;
      }
      (signedShoelace(path) < 0 ? outers : holes).add(piece);
    }
    List<Rectangle2D> outerBounds = new ArrayList<>();
    List<Double> outerSizes = new ArrayList<>();
    for (Area outer : outers) {
      outerBounds.add(outer.getBounds2D());
      outerSizes.add(shoelace(outer));
    }
    List<Area> islands = new ArrayList<>();
    for (Area outer : outers) {
      islands.add(new Area(outer));
    }
    for (Area hole : holes) {
      Rectangle2D b = hole.getBounds2D();
      int best = -1;
      for (int i = 0; i < outers.size(); i++) {
        if (!outerBounds.get(i).contains(b)
            || (best >= 0 && outerSizes.get(i) >= outerSizes.get(best))) {
          continue;
        }
        Area rest = new Area(hole);
        rest.subtract(outers.get(i));
        if (rest.isEmpty()) {
          best = i;
        }
      }
      if (best >= 0) {
        islands.get(best).subtract(hole);
      }
    }
    islands.removeIf(Area::isEmpty);
    islands.sort(
        Comparator.comparingDouble((Area a) -> a.getBounds2D().getMinY())
            .thenComparingDouble(a -> a.getBounds2D().getMinX()));
    return islands;
  }

  /** The closed subpaths of {@code area}'s path, with their curves. */
  static List<Path2D.Double> subpaths(Area area) {
    List<Path2D.Double> result = new ArrayList<>();
    PathIterator it = area.getPathIterator(null);
    Path2D.Double current = null;
    double[] c = new double[6];
    while (!it.isDone()) {
      switch (it.currentSegment(c)) {
        case PathIterator.SEG_MOVETO -> {
          current = new Path2D.Double();
          current.moveTo(c[0], c[1]);
          result.add(current);
        }
        case PathIterator.SEG_LINETO -> current.lineTo(c[0], c[1]);
        case PathIterator.SEG_QUADTO -> current.quadTo(c[0], c[1], c[2], c[3]);
        case PathIterator.SEG_CUBICTO -> current.curveTo(c[0], c[1], c[2], c[3], c[4], c[5]);
        case PathIterator.SEG_CLOSE -> current.closePath();
        default -> {}
      }
      it.next();
    }
    return result;
  }

  static String ids(List<Item> items) {
    List<Integer> ids = new ArrayList<>();
    for (Item item : items) {
      ids.add(item.getId());
    }
    ids.sort(null);
    StringBuilder text = new StringBuilder("[");
    for (int id : ids) {
      if (text.length() > 1) {
        text.append(',');
      }
      text.append(id);
    }
    return text.append(']').toString();
  }

  static Boards.Line bounds(Boards.Line line, Area area) {
    Rectangle2D b = area.getBounds2D();
    return line.dbl("minX", b.getMinX())
        .dbl("minY", b.getMinY())
        .dbl("maxX", b.getMaxX())
        .dbl("maxY", b.getMaxY());
  }

  /** The absolute sum of the signed shoelace areas of the flattened subpaths. */
  static double shoelace(Area area) {
    if (area == null || area.isEmpty()) {
      return 0.0;
    }
    return Math.abs(signedShoelace(area));
  }

  /** The sum of the signed shoelace areas of the flattened subpaths of {@code shape}. */
  static double signedShoelace(java.awt.Shape shape) {
    PathIterator it = shape.getPathIterator(null, FLATNESS);
    double[] c = new double[6];
    double sum = 0.0;
    double startX = 0.0;
    double startY = 0.0;
    double lastX = 0.0;
    double lastY = 0.0;
    while (!it.isDone()) {
      int type = it.currentSegment(c);
      if (type == PathIterator.SEG_MOVETO) {
        startX = lastX = c[0];
        startY = lastY = c[1];
      } else if (type == PathIterator.SEG_LINETO) {
        sum += lastX * c[1] - c[0] * lastY;
        lastX = c[0];
        lastY = c[1];
      } else if (type == PathIterator.SEG_CLOSE) {
        sum += lastX * startY - startX * lastY;
        lastX = startX;
        lastY = startY;
      }
      it.next();
    }
    return sum / 2.0;
  }

  /**
   * A copy of {@code DesignRulesChecker.decomposeAreaIntoIslands} and {@code mergeOrAddIsland} at
   * the pin that also counts, per island, the subpaths merged by {@code exclusiveOr}. {@code
   * writeArea} checks that it gives the same islands as the original.
   */
  static List<Area> replayIslands(Area area, List<Integer> xors) {
    List<Area> islands = new ArrayList<>();
    if (area == null || area.isEmpty()) {
      return islands;
    }
    PathIterator it = area.getPathIterator(null);
    Path2D.Double current = null;
    double[] c = new double[6];
    while (!it.isDone()) {
      int type = it.currentSegment(c);
      switch (type) {
        case PathIterator.SEG_MOVETO -> {
          if (current != null) {
            merge(islands, xors, new Area(current));
          }
          current = new Path2D.Double();
          current.moveTo(c[0], c[1]);
        }
        case PathIterator.SEG_LINETO -> {
          if (current != null) {
            current.lineTo(c[0], c[1]);
          }
        }
        case PathIterator.SEG_QUADTO -> {
          if (current != null) {
            current.quadTo(c[0], c[1], c[2], c[3]);
          }
        }
        case PathIterator.SEG_CUBICTO -> {
          if (current != null) {
            current.curveTo(c[0], c[1], c[2], c[3], c[4], c[5]);
          }
        }
        case PathIterator.SEG_CLOSE -> {
          if (current != null) {
            current.closePath();
          }
        }
        default -> {}
      }
      it.next();
    }
    if (current != null) {
      merge(islands, xors, new Area(current));
    }
    return islands;
  }

  static void merge(List<Area> islands, List<Integer> xors, Area piece) {
    if (piece.isEmpty()) {
      return;
    }
    for (int i = 0; i < islands.size(); i++) {
      Area existing = islands.get(i);
      Area intersection = new Area(existing);
      intersection.intersect(piece);
      if (!intersection.isEmpty()) {
        Area testSubtract = new Area(piece);
        testSubtract.subtract(existing);
        if (testSubtract.isEmpty()) {
          existing.subtract(piece);
        } else {
          existing.exclusiveOr(piece);
          xors.set(i, xors.get(i) + 1);
        }
        return;
      }
    }
    islands.add(piece);
    xors.add(0);
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
