import app.freerouting.Freerouting;
import app.freerouting.board.actions.ItemSelectionFilter;
import app.freerouting.board.facade.RoutingBoard;
import app.freerouting.board.model.items.Connectable;
import app.freerouting.board.model.items.DrillItem;
import app.freerouting.board.model.items.Item;
import app.freerouting.board.model.items.ObstacleArea;
import app.freerouting.board.model.items.Pin;
import app.freerouting.board.model.structure.AngleRestriction;
import app.freerouting.board.model.structure.FixedState;
import app.freerouting.board.trace.PolylineTrace;
import app.freerouting.core.RoutingJob;
import app.freerouting.geometry.planar.Area;
import app.freerouting.geometry.planar.Circle;
import app.freerouting.geometry.planar.ConvexShape;
import app.freerouting.geometry.planar.FloatPoint;
import app.freerouting.geometry.planar.IntBox;
import app.freerouting.geometry.planar.IntOctagon;
import app.freerouting.geometry.planar.IntPoint;
import app.freerouting.geometry.planar.Line;
import app.freerouting.geometry.planar.Point;
import app.freerouting.geometry.planar.PolygonShape;
import app.freerouting.geometry.planar.PolylineArea;
import app.freerouting.geometry.planar.PolylineShape;
import app.freerouting.geometry.planar.Shape;
import app.freerouting.geometry.planar.Side;
import app.freerouting.geometry.planar.TileShape;
import app.freerouting.io.specctra.SesReader;
import app.freerouting.management.BoardLoader;
import app.freerouting.rules.BoardRules;
import app.freerouting.settings.GlobalSettings;
import java.io.FileInputStream;
import java.io.InputStream;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.Collection;
import java.util.Iterator;
import java.util.LinkedList;
import java.util.List;
import java.util.Random;
import java.util.Set;
import java.util.TreeSet;
import java.util.UUID;

/**
 * Board probe {@code editor} (I03.7): the board calls of the editor states, headless, mirrored by
 * {@code crates/freerouting/tests/interactive_editor.rs}.
 *
 * <p>The states need a {@code GuiBoardManager} (Swing), so this probe transcribes their few board
 * calls: {@code GuiBoardInteractionController.pickItems} (the current layer 0, then every other
 * layer, all visible), {@code InspectedItemState.extentToWholeNets/Components/ConnectedSets/
 * Connections} and {@code filter} with the default {@code new ItemSelectionFilter()}, {@code
 * InspectItemsInRegionState.selectAllInRegion} (select on all visible layers on and off, all
 * layers visible), {@code PinSwapState.complete} ({@code checkChangeNet} both ways, snapshot,
 * {@code Pin.swap}), and the construction states {@code CornerItemConstructionState}, {@code
 * TileConstructionState}, {@code PolygonShapeConstructionState}, {@code CircleConstructionState}
 * and {@code HoleConstructionState} ({@code snap}, {@code addCorner}, {@code
 * removeConcaveCorners}, {@code removeConcaveCornersAtClose}, {@code addCornerForSnapAngle},
 * {@code checkShape} + {@code insertObstacle}, the hole's remove and reinsert). Current layer 0.
 *
 * <p>Boards: J2 and DAC2020_bm05 with their fixtures.tsv session (stage {@code ses}), and the
 * pin-swap board: {@code Issue143-rpi_splitter} with {@link #PART_LIBRARY} inserted before its
 * {@code (network} scope (stage {@code pinswap}; for the refusal, the same text with the session
 * imported, stage {@code pinswap-ses}). Every other row exits 3.
 *
 * <p>One {@code new Random(SEED)} per board draws, in this order: {@value #SELECTIONS} items
 * (highest id first) whose single-item selection is extended four ways and filtered, and picked at
 * their location; {@value #REGIONS} region boxes around drawn items; then for each angle
 * restriction (none, 45, 90, set on the rules and restored afterwards) a circle, a tile and a
 * polygon near drawn items, a hole in each inserted tile or polygon, and a circle on a drawn pin's
 * centre. Item sets are written highest id first; {@code -} is Java's return to the previous state
 * (an empty set before the filter).
 */
public class EditorProbe {
  static final long SEED = 20261016L;
  static final int SELECTIONS = 12;
  static final int REGIONS = 6;
  static final String PART_LIBRARY =
      "  (part_library\n"
          + "    (logical_part_mapping \"USB-MINIB-SWAP\" (component \"J3\"))\n"
          + "    (logical_part \"USB-MINIB-SWAP\"\n"
          + "      (pin \"D+\" 1 \"A\" 0 \"1\" 1)\n"
          + "      (pin \"D-\" 2 \"A\" 0 \"2\" 1)\n"
          + "      (pin \"ID\" 3 \"A\" 0 \"3\" 1)\n"
          + "      (pin \"VBUS\" 4 \"A\" 0 \"4\" 1)\n"
          + "    )\n"
          + "  )\n";
  static final String SWAP_BOARD = "Issue143-rpi_splitter";

  public static void main(String[] args) throws Exception {
    String board = Boards.board(args[0]);
    boolean swapBoard = board.equals(SWAP_BOARD);
    if (!swapBoard
        && !board.equals("Issue026-J2_reference")
        && !board.equals("Issue508-DAC2020_bm05")) {
      System.exit(3);
    }
    Boards.Out out = new Boards.Out("editor", args[0]);
    try {
      long started = System.nanoTime();
      if (swapBoard) {
        Path dsn = Files.createTempFile("editor-pinswap", ".dsn");
        String text = Files.readString(Path.of(args[0]), StandardCharsets.UTF_8);
        int at = text.indexOf("  (network");
        Files.writeString(
            dsn, text.substring(0, at) + PART_LIBRARY + text.substring(at), StandardCharsets.UTF_8);
        RoutingBoard routingBoard = Boards.load(dsn.toString());
        new EditorProbe(out, routingBoard, "pinswap").script(true);
        RoutingBoard routed = Boards.load(dsn.toString());
        Boards.importSession(routed, args[1]);
        new EditorProbe(out, routed, "pinswap-ses").refusals();
        Files.deleteIfExists(dsn);
      } else {
        RoutingBoard routingBoard = Boards.load(args[0]);
        Boards.importSession(routingBoard, args[1]);
        new EditorProbe(out, routingBoard, "ses").script(false);
      }
      System.err.printf("editor: %.1f ms%n", (System.nanoTime() - started) / 1e6);
    } catch (Throwable t) {
      out.exception(t);
    }
    out.write(args[2]);
  }

  final Boards.Out out;
  final RoutingBoard board;
  final String stage;
  final Random random = new Random(SEED);
  final ItemSelectionFilter filter = new ItemSelectionFilter();

  EditorProbe(Boards.Out out, RoutingBoard board, String stage) {
    this.out = out;
    this.board = board;
    this.stage = stage;
  }

  Boards.Line line(String phase) {
    return new Boards.Line().str("stage", stage).str("phase", phase);
  }

  List<Item> sortedItems() {
    List<Item> items = new ArrayList<>(board.getItems());
    items.sort((a, b) -> Integer.compare(b.getId(), a.getId()));
    return items;
  }

  static String ids(Collection<Item> items) {
    if (items == null) {
      return "-";
    }
    List<Integer> values = new ArrayList<>();
    for (Item item : items) {
      values.add(item.getId());
    }
    values.sort((a, b) -> Integer.compare(b, a));
    StringBuilder text = new StringBuilder();
    for (int v : values) {
      text.append(text.length() > 0 ? "," : "").append(v);
    }
    return text.toString();
  }

  /** The point a selection or keepout is drawn at: a drill centre or a trace's first corner. */
  static FloatPoint location(Item item) {
    if (item instanceof DrillItem drill) {
      return drill.getCenter().toFloat();
    }
    if (item instanceof PolylineTrace trace) {
      return trace.polyline().corner(0).toFloat();
    }
    IntBox box = item.boundingBox();
    return new FloatPoint((box.ll.x + (double) box.ur.x) / 2, (box.ll.y + (double) box.ur.y) / 2);
  }

  void script(boolean pinSwap) {
    selections();
    regions();
    if (pinSwap) {
      swaps();
    }
    keepouts();
  }

  // ---------------------------------------------------------------------------------------------
  // Selection
  // ---------------------------------------------------------------------------------------------

  /** {@code GuiBoardInteractionController.pickItems} with current layer 0, all layers visible. */
  Set<Item> pickItems(FloatPoint at, ItemSelectionFilter itemFilter) {
    IntPoint location = at.round();
    Set<Item> result = board.pickItems(location, 0, itemFilter);
    if (result.isEmpty()) {
      for (int i = 0; i < board.getLayerCount(); i++) {
        if (i == 0) {
          continue;
        }
        result.addAll(board.pickItems(location, i, itemFilter));
      }
    }
    return result;
  }

  Set<Item> extentToWholeNets(Set<Item> itemList) {
    Set<Integer> nets = new TreeSet<>();
    for (Item item : itemList) {
      if (item instanceof Connectable) {
        for (int i = 0; i < item.netCount(); i++) {
          nets.add(item.getNetNumber(i));
        }
      }
    }
    Set<Item> result = new TreeSet<>();
    for (int net : nets) {
      result.addAll(board.getConnectableItems(net));
    }
    return result.isEmpty() ? null : filter.filter(result);
  }

  Set<Item> extentToWholeComponents(Set<Item> itemList) {
    Set<Integer> groups = new TreeSet<>();
    for (Item item : itemList) {
      if (item.getComponentId() > 0) {
        groups.add(item.getComponentId());
      }
    }
    Set<Item> result = new TreeSet<>(itemList);
    for (int group : groups) {
      result.addAll(board.getComponentItems(group));
    }
    return result.isEmpty() ? null : result;
  }

  Set<Item> extentToWholeConnectedSets(Set<Item> itemList) {
    Set<Item> result = new TreeSet<>();
    for (Item item : itemList) {
      if (item instanceof Connectable) {
        result.addAll(item.getConnectedSet(-1));
      }
    }
    return result.isEmpty() ? null : filter.filter(result);
  }

  Set<Item> extentToWholeConnections(Set<Item> itemList) {
    Set<Item> result = new TreeSet<>();
    for (Item item : itemList) {
      if (item instanceof Connectable) {
        result.addAll(item.getConnectionItems());
      }
    }
    return result.isEmpty() ? null : filter.filter(result);
  }

  void selections() {
    List<Item> items = sortedItems();
    for (int s = 0; s < SELECTIONS; s++) {
      Item item = items.get(random.nextInt(items.size()));
      Set<Item> single = new TreeSet<>();
      single.add(item);
      Set<Item> filtered = filter.filter(single);
      out.line(
          line("select")
              .num("item", item.getId())
              .str("pick", ids(pickItems(location(item), filter)))
              .str("nets", ids(extentToWholeNets(single)))
              .str("components", ids(extentToWholeComponents(single)))
              .str("connectedSets", ids(extentToWholeConnectedSets(single)))
              .str("connections", ids(extentToWholeConnections(single)))
              .str("filter", ids(filtered.isEmpty() ? null : filtered))
              .end());
    }
  }

  /** {@code InspectItemsInRegionState.selectAllInRegion}'s search, all layers visible. */
  Set<Item> selectAllInRegion(IntBox box, boolean allVisible) {
    int selectLayer = allVisible ? -1 : 0;
    Set<Item> found = filter.filter(board.overlappingItems(box, selectLayer));
    if (allVisible) {
      Set<Item> visible = new TreeSet<>();
      for (Item item : found) {
        for (int i = item.firstLayer(); i <= item.lastLayer(); i++) {
          visible.add(item);
          break;
        }
      }
      found = visible;
    }
    return found;
  }

  void regions() {
    List<Item> items = sortedItems();
    for (int r = 0; r < REGIONS; r++) {
      FloatPoint centre = location(items.get(random.nextInt(items.size())));
      double w = 100 + random.nextInt(3000) + 0.5;
      double h = 100 + random.nextInt(3000) + 0.25;
      FloatPoint c1 = new FloatPoint(centre.x - w, centre.y - h);
      FloatPoint c2 = new FloatPoint(centre.x + w, centre.y + h);
      IntPoint p1 = c1.round();
      IntPoint p2 = c2.round();
      IntBox box =
          new IntBox(
              Math.min(p1.x, p2.x), Math.min(p1.y, p2.y), Math.max(p1.x, p2.x), Math.max(p1.y, p2.y));
      out.line(
          line("region")
              .str("box", box.ll.x + " " + box.ll.y + " " + box.ur.x + " " + box.ur.y)
              .str("allLayers", ids(selectAllInRegion(box, true)))
              .str("layer0", ids(selectAllInRegion(box, false)))
              .end());
    }
  }

  // ---------------------------------------------------------------------------------------------
  // Pin swap
  // ---------------------------------------------------------------------------------------------

  List<Pin> pinsOf(String component) {
    List<Pin> pins = new ArrayList<>();
    for (Item item : sortedItems()) {
      if (item instanceof Pin pin
          && pin.getComponentId() > 0
          && board.components.get(pin.getComponentId()).name.equals(component)) {
        pins.add(pin);
      }
    }
    return pins;
  }

  static String nets(Item item) {
    StringBuilder text = new StringBuilder();
    for (int i = 0; i < item.netCount(); i++) {
      text.append(i > 0 ? "," : "").append(item.getNetNumber(i));
    }
    return text.toString();
  }

  /** {@code PinSwapState.complete}; returns the status key ({@code -}: none). */
  String completeSwap(Pin fromPin, Pin toPin) {
    if (fromPin.netCount() > 1 || toPin.netCount() > 1) {
      return "-";
    }
    int fromNetNumber = fromPin.netCount() > 0 ? fromPin.getNetNumber(0) : -1;
    int toNetNumber = toPin.netCount() > 0 ? toPin.getNetNumber(0) : -1;
    if (!board.checkChangeNet(fromPin, toNetNumber)) {
      return "pin_not_swapped_because_it_is_already_connected";
    }
    if (!board.checkChangeNet(toPin, fromNetNumber)) {
      return "pin_not_swapped_because_second_pin_is_already_connected";
    }
    board.generateSnapshot();
    fromPin.swap(toPin);
    return "pin_swap_completed";
  }

  void pinDump() {
    StringBuilder text = new StringBuilder();
    for (Item item : sortedItems()) {
      if (item instanceof Pin pin) {
        text.append(text.length() > 0 ? " " : "").append(pin.getId()).append(':').append(nets(pin));
      }
    }
    out.line(line("pins").str("nets", text.toString()).end());
  }

  void swaps() {
    for (Pin from : pinsOf("J3")) {
      List<Pin> swappable = new ArrayList<>(from.getSwappablePins());
      swappable.sort((a, b) -> Integer.compare(b.getId(), a.getId()));
      out.line(
          line("swappable")
              .num("pin", from.getId())
              .str("pins", ids(new ArrayList<>(swappable)))
              .end());
    }
    for (Pin from : pinsOf("J3")) {
      List<Pin> swappable = new ArrayList<>(from.getSwappablePins());
      swappable.sort((a, b) -> Integer.compare(b.getId(), a.getId()));
      for (Pin to : swappable) {
        String key = completeSwap(from, to);
        out.line(
            line("swap")
                .num("from", from.getId())
                .num("to", to.getId())
                .str("status", key)
                .str("fromNets", nets(from))
                .str("toNets", nets(to))
                .end());
      }
    }
    pinDump();
  }

  void refusals() {
    for (Pin from : pinsOf("J3")) {
      List<Pin> swappable = new ArrayList<>(from.getSwappablePins());
      swappable.sort((a, b) -> Integer.compare(b.getId(), a.getId()));
      for (Pin to : swappable) {
        String key = completeSwap(from, to);
        out.line(
            line("swap")
                .num("from", from.getId())
                .num("to", to.getId())
                .str("status", key)
                .str("fromNets", nets(from))
                .str("toNets", nets(to))
                .end());
      }
    }
    pinDump();
  }

  // ---------------------------------------------------------------------------------------------
  // Keepouts
  // ---------------------------------------------------------------------------------------------

  /** The construction states' corner list and their board calls. */
  final class Construction {
    LinkedList<IntPoint> cornerList = new LinkedList<>();

    AngleRestriction restriction() {
      return board.rules.getTraceAngleRestriction();
    }

    IntPoint snap(IntPoint point) {
      boolean listEmpty = cornerList.isEmpty();
      if (restriction() == AngleRestriction.NINETY_DEGREE && !listEmpty) {
        return point.orthogonalProjection(cornerList.getLast());
      } else if (restriction() == AngleRestriction.FORTYFIVE_DEGREE && !listEmpty) {
        return point.fortyfiveDegreeProjection(cornerList.getLast());
      }
      return point;
    }

    void addCorner(FloatPoint location) {
      cornerList.add(snap(location.round()));
    }

    void addCornerForSnapAngle() {
      if (restriction() == AngleRestriction.NONE) {
        return;
      }
      IntPoint firstCorner = cornerList.getFirst();
      IntPoint lastCorner = cornerList.getLast();
      IntPoint addCorner = null;
      if (restriction() == AngleRestriction.NINETY_DEGREE) {
        addCorner = lastCorner.ninetyDegreeCorner(firstCorner, true);
      } else if (restriction() == AngleRestriction.FORTYFIVE_DEGREE) {
        addCorner = lastCorner.fortyfiveDegreeCorner(firstCorner, true);
      }
      if (addCorner != null) {
        cornerList.add(addCorner);
      }
    }

    void removeConcaveCorners() {
      IntPoint[] corners = cornerList.toArray(new IntPoint[0]);
      int newLength = corners.length;
      if (newLength < 3) {
        return;
      }
      IntPoint lastCorner = corners[newLength - 1];
      IntPoint currentCorner = corners[newLength - 2];
      while (newLength > 2) {
        IntPoint prevCorner = corners[newLength - 3];
        Side lastCornerSide = lastCorner.sideOf(prevCorner, currentCorner);
        if (lastCornerSide == Side.ON_THE_LEFT) {
          break;
        }
        if (restriction() != AngleRestriction.FORTYFIVE_DEGREE) {
          corners[newLength - 2] = lastCorner;
        }
        --newLength;
        if (restriction() == AngleRestriction.NINETY_DEGREE) {
          IntPoint prevPrevCorner = null;
          if (newLength >= 3) {
            prevPrevCorner = corners[newLength - 3];
          }
          if (prevPrevCorner != null && prevPrevCorner.x == prevCorner.x) {
            corners[newLength - 2] = new IntPoint(prevCorner.x, lastCorner.y);
          } else {
            corners[newLength - 2] = new IntPoint(lastCorner.x, prevCorner.y);
          }
        }
        currentCorner = prevCorner;
      }
      if (newLength < corners.length) {
        cornerList = new LinkedList<>(Arrays.asList(corners).subList(0, newLength));
      }
    }

    void removeConcaveCornersAtClose() {
      addCornerForSnapAngle();
      if (cornerList.size() < 4) {
        return;
      }
      IntPoint[] corners = cornerList.toArray(new IntPoint[0]);
      int newLength = corners.length;
      IntPoint firstCorner = corners[0];
      IntPoint secondCorner = corners[1];
      while (newLength > 3) {
        IntPoint lastCorner = corners[newLength - 1];
        if (lastCorner.sideOf(secondCorner, firstCorner) != Side.ON_THE_LEFT) {
          break;
        }
        --newLength;
      }
      if (newLength != corners.length) {
        cornerList = new LinkedList<>(Arrays.asList(corners).subList(0, newLength));
        addCornerForSnapAngle();
      }
    }

    /** {@code TileConstructionState}: start, clicks, complete. */
    ObstacleArea tile(List<FloatPoint> clicks) {
      addCorner(clicks.get(0));
      for (int i = 1; i < clicks.size(); i++) {
        addCorner(clicks.get(i));
        removeConcaveCorners();
      }
      removeConcaveCornersAtClose();
      int cornerCount = cornerList.size();
      if (cornerCount <= 2) {
        return null;
      }
      Line[] edgeLines = new Line[cornerCount];
      Iterator<IntPoint> it = cornerList.iterator();
      IntPoint firstCorner = it.next();
      IntPoint prevCorner = firstCorner;
      for (int i = 0; i < cornerCount - 1; i++) {
        IntPoint nextCorner = it.next();
        edgeLines[i] = new Line(prevCorner, nextCorner);
        prevCorner = nextCorner;
      }
      edgeLines[cornerCount - 1] = new Line(prevCorner, firstCorner);
      return insert(TileShape.getInstance(edgeLines));
    }

    /** {@code PolygonShapeConstructionState}: start, clicks, complete. */
    ObstacleArea polygon(List<FloatPoint> clicks) {
      for (FloatPoint click : clicks) {
        addCorner(click);
      }
      addCornerForSnapAngle();
      int cornerCount = cornerList.size();
      if (cornerCount <= 2) {
        return null;
      }
      PolygonShape obstacleShape = new PolygonShape(cornerList.toArray(new IntPoint[0]));
      if (obstacleShape.splitToConvex() == null) {
        return null;
      }
      return insert(obstacleShape);
    }

    /** {@code CircleConstructionState.complete}. */
    ObstacleArea circle(FloatPoint circleCenter, double circleRadius) {
      IntPoint center = circleCenter.round();
      int radius = (int) Math.round(circleRadius);
      if (!(circleRadius > 0)) {
        return null;
      }
      ConvexShape obstacleShape = new Circle(center, radius);
      if (restriction() == AngleRestriction.NINETY_DEGREE) {
        obstacleShape = obstacleShape.boundingBox();
      } else if (restriction() == AngleRestriction.FORTYFIVE_DEGREE) {
        obstacleShape = obstacleShape.boundingOctagon();
      }
      return insert(obstacleShape);
    }

    ObstacleArea insert(Area shape) {
      int clearanceClassIndex = BoardRules.clearanceClassNone();
      if (!board.checkShape(shape, 0, new int[0], clearanceClassIndex)) {
        return null;
      }
      board.generateSnapshot();
      return board.insertObstacle(shape, 0, clearanceClassIndex, FixedState.UNFIXED);
    }

    /** {@code HoleConstructionState}: startOk, clicks, complete; the status key. */
    String hole(List<FloatPoint> clicks, Item[] inserted) {
      ItemSelectionFilter.SelectableChoices[] selectableChoices = {
        ItemSelectionFilter.SelectableChoices.KEEPOUT,
        ItemSelectionFilter.SelectableChoices.VIA_KEEPOUT,
        ItemSelectionFilter.SelectableChoices.CONDUCTION
      };
      Collection<Item> foundItems =
          board.pickItems(clicks.get(0).round(), 0, new ItemSelectionFilter(selectableChoices));
      if (foundItems.size() != 1) {
        return "no_item_found_for_adding_hole";
      }
      Item foundItem = foundItems.iterator().next();
      if (!(foundItem instanceof ObstacleArea itemToModify)) {
        return "no_obstacle_area_found_for_adding_hole";
      }
      if (itemToModify.getArea() instanceof Circle) {
        return "adding_hole_to_circle_not_yet_implemented";
      }
      addCorner(clicks.get(0));
      for (int i = 1; i < clicks.size(); i++) {
        if (itemToModify.getArea().contains(clicks.get(i))) {
          addCorner(clicks.get(i));
        }
      }
      addCornerForSnapAngle();
      int cornerCount = cornerList.size();
      if (cornerCount <= 2) {
        return "adding_hole_failed";
      }
      Area obsArea = itemToModify.getArea();
      Shape[] oldHoles = obsArea.getHoles();
      PolylineShape newBorder = (PolylineShape) obsArea.getBorder();
      if (newBorder == null) {
        return "adding_hole_failed";
      }
      PolylineShape[] newHoles = new PolylineShape[oldHoles.length + 1];
      for (int i = 0; i < oldHoles.length; i++) {
        newHoles[i] = (PolylineShape) oldHoles[i];
        if (newHoles[i] == null) {
          return "adding_hole_failed";
        }
      }
      newHoles[newHoles.length - 1] = new PolygonShape(cornerList.toArray(new IntPoint[0]));
      PolylineArea newObsArea = new PolylineArea(newBorder, newHoles);
      if (newObsArea.splitToConvex() == null) {
        return "adding_hole_failed";
      }
      board.generateSnapshot();
      board.removeItem(itemToModify);
      inserted[0] =
          board.insertObstacle(
              newObsArea,
              itemToModify.getLayer(),
              itemToModify.clearanceClassIndex(),
              FixedState.UNFIXED);
      return "adding_hole_completed";
    }
  }

  /** A tile shape as the board-queries probe writes it. */
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

  static String point(Point p) {
    if (p instanceof IntPoint ip) {
      return ip.x + "," + ip.y;
    }
    FloatPoint f = p.toFloat();
    return "f" + Boards.bits(f.x) + "," + Boards.bits(f.y);
  }

  static String tiles(Item item) {
    if (item == null) {
      return "-";
    }
    StringBuilder text = new StringBuilder();
    for (int i = 0; i < item.tileShapeCount(); i++) {
      text.append(i > 0 ? "|" : "").append(shape(item.getTileShape(i)));
    }
    return text.toString();
  }

  static String clicks(List<FloatPoint> points) {
    StringBuilder text = new StringBuilder();
    for (FloatPoint p : points) {
      text.append(text.length() > 0 ? " " : "")
          .append(Boards.bits(p.x))
          .append(',')
          .append(Boards.bits(p.y));
    }
    return text.toString();
  }

  static final int[][] DIAMOND = {{1000, 0}, {0, 1000}, {0, 0}, {-1000, 0}, {0, -1000}};
  static final int[][] STAR = {
    {1000, 0}, {500, 866}, {-500, 866}, {-1000, 0}, {-500, -866}, {500, -866}
  };

  /**
   * Clicks around {@code centre}: the directions of {@code dirs} (thousandths) times a drawn radius
   * in [{@code size / 2}, {@code size}], every second one at 0.4 of it when {@code star}.
   */
  List<FloatPoint> drawClicks(FloatPoint centre, int size, int[][] dirs, boolean star) {
    List<FloatPoint> points = new ArrayList<>();
    for (int i = 0; i < dirs.length; i++) {
      double r = size / 2 + random.nextInt(size / 2 + 1);
      if (star && i % 2 == 1) {
        r = r * 0.4;
      }
      points.add(new FloatPoint(centre.x + dirs[i][0] * r / 1000, centre.y + dirs[i][1] * r / 1000));
    }
    return points;
  }

  /** A point of the board box, off the integer grid. */
  FloatPoint drawPoint(IntBox box) {
    double x = box.ll.x + random.nextInt(box.ur.x - box.ll.x + 1) + 0.25;
    double y = box.ll.y + random.nextInt(box.ur.y - box.ll.y + 1) - 0.5;
    return new FloatPoint(x, y);
  }

  void keepoutLine(String kind, AngleRestriction restriction, List<FloatPoint> input, Item item) {
    out.line(
        line("keepout")
            .str("kind", kind)
            .str("angle", restriction.name())
            .str("clicks", clicks(input))
            .num("id", item == null ? -1 : item.getId())
            .str("tiles", tiles(item))
            .num("maxGeneratedId", board.communication.idGenerator.maxGeneratedId())
            .end());
  }

  void hole(AngleRestriction restriction, Item area) {
    if (area == null) {
      return;
    }
    IntBox box = area.boundingBox();
    double cx = (box.ll.x + (double) box.ur.x) / 2;
    double cy = (box.ll.y + (double) box.ur.y) / 2;
    double d = Math.max(1, Math.min(box.ur.x - box.ll.x, box.ur.y - box.ll.y) / 6) + 0.5;
    List<FloatPoint> points =
        List.of(
            new FloatPoint(cx - d, cy - d),
            new FloatPoint(cx + d, cy - d),
            new FloatPoint(cx + d, cy + d),
            new FloatPoint(cx - d, cy + d));
    Item[] inserted = new Item[1];
    String key = new Construction().hole(points, inserted);
    out.line(
        line("hole")
            .str("angle", restriction.name())
            .num("area", area.getId())
            .str("status", key)
            .num("id", inserted[0] == null ? -1 : inserted[0].getId())
            .str("tiles", tiles(inserted[0]))
            .end());
  }

  void keepouts() {
    AngleRestriction own = board.rules.getTraceAngleRestriction();
    List<Item> items = sortedItems();
    List<Pin> pins = new ArrayList<>();
    for (Item item : items) {
      if (item instanceof Pin pin) {
        pins.add(pin);
      }
    }
    AngleRestriction[] restrictions = {
      AngleRestriction.NONE, AngleRestriction.FORTYFIVE_DEGREE, AngleRestriction.NINETY_DEGREE
    };
    IntBox box = board.getBoundingBox();
    int size = Math.max(Math.min(box.ur.x - box.ll.x, box.ur.y - box.ll.y) / 30, 10);
    for (AngleRestriction restriction : restrictions) {
      board.rules.setTraceAngleRestriction(restriction);
      for (int k = 0; k < 3; k++) {
        FloatPoint circleCentre = drawPoint(box);
        double radius = size / 4 + random.nextInt(size / 2 + 1) + 0.5;
        ObstacleArea circle = new Construction().circle(circleCentre, radius);
        keepoutLine("circle", restriction, List.of(circleCentre, new FloatPoint(radius, 0)), circle);
        List<FloatPoint> tileClicks = drawClicks(drawPoint(box), size, DIAMOND, false);
        ObstacleArea tile = new Construction().tile(tileClicks);
        keepoutLine("tile", restriction, tileClicks, tile);
        hole(restriction, tile);
        List<FloatPoint> polygonClicks = drawClicks(drawPoint(box), size, STAR, true);
        ObstacleArea polygon = new Construction().polygon(polygonClicks);
        keepoutLine("polygon", restriction, polygonClicks, polygon);
        hole(restriction, polygon);
      }
      if (!pins.isEmpty()) {
        Pin pin = pins.get(random.nextInt(pins.size()));
        FloatPoint centre = pin.getCenter().toFloat();
        ObstacleArea overlap = new Construction().circle(centre, 100.0);
        keepoutLine("overlap", restriction, List.of(centre, new FloatPoint(100.0, 0)), overlap);
      }
    }
    board.rules.setTraceAngleRestriction(own);
    out.line(
        line("end")
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
