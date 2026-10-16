import app.freerouting.Freerouting;
import app.freerouting.autoroute.expansion.CompleteFreeSpaceExpansionRoom;
import app.freerouting.board.actions.ComponentItemTransform;
import app.freerouting.board.facade.RoutingBoard;
import app.freerouting.board.model.items.ConductionArea;
import app.freerouting.board.model.items.DrillItem;
import app.freerouting.board.model.items.Item;
import app.freerouting.board.model.items.ObstacleArea;
import app.freerouting.board.model.items.Pin;
import app.freerouting.board.model.items.Trace;
import app.freerouting.board.model.items.Via;
import app.freerouting.board.model.structure.Component;
import app.freerouting.board.model.structure.FixedState;
import app.freerouting.board.model.structure.LayerStructure;
import app.freerouting.board.searchtree.ShapeSearchTree;
import app.freerouting.board.trace.PolylineTrace;
import app.freerouting.core.RoutingJob;
import app.freerouting.core.library.Package;
import app.freerouting.core.library.Padstack;
import app.freerouting.drc.ClearanceViolation;
import app.freerouting.geometry.planar.Area;
import app.freerouting.geometry.planar.ConvexShape;
import app.freerouting.geometry.planar.FloatPoint;
import app.freerouting.datastructures.ShapeTree;
import app.freerouting.geometry.planar.IntBox;
import app.freerouting.geometry.planar.IntOctagon;
import app.freerouting.geometry.planar.IntPoint;
import app.freerouting.geometry.planar.Point;
import app.freerouting.geometry.planar.Polyline;
import app.freerouting.geometry.planar.RegularTileShape;
import app.freerouting.geometry.planar.PolylineArea;
import app.freerouting.geometry.planar.PolylineShape;
import app.freerouting.geometry.planar.Shape;
import app.freerouting.geometry.planar.Vector;
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
import java.util.Arrays;
import java.util.Collection;
import java.util.Deque;
import java.util.LinkedList;
import java.util.List;
import java.util.Map;
import java.util.Random;
import java.util.Set;
import java.util.TreeMap;
import java.util.TreeSet;
import java.util.UUID;

/**
 * Board probe {@code transform} (I04): the move, turn, mirror and copy of a selection, headless,
 * mirrored by {@code crates/freerouting/tests/interactive_transform.rs}.
 *
 * <p>{@code MoveItemState} and {@code CopyItemState} need a {@code GuiBoardManager} (Swing), so
 * this probe transcribes their board calls: {@code MoveItemState.getInstance} (the extension to
 * whole components and connected sets, the fixed-state and connectivity checks, the constructor's
 * snapshot, {@code detachItemForMove} and {@code copy(0)}), {@code move} with {@code
 * adjustToPlacementGrid}, {@code turn90Degree}, {@code rotate}, {@code changePlacementSide},
 * {@code resetRotation}, {@code transformItems} (the real {@link ComponentItemTransform}), {@code
 * complete} and {@code cancel}; and {@code CopyItemState}'s constructor, {@code changePosition},
 * {@code changeLayerAction}, {@code changePadstackLayers} and {@code insert}. The ratsnest, the
 * observers and the drawing are left out. The settings are fixed: current layer 0, no component
 * grid except where a trial says so.
 *
 * <p>Boards: J2 and DAC2020_bm05 with their fixtures.tsv session, and
 * Issue955-rotated-pad-contacts as loaded; every other row exits 3. Each board runs the script on
 * the board as loaded (stage {@code load}) and, if it has a session, on a fresh board with the
 * session imported (stage {@code ses}). One {@code new Random(SEED)} per board draws, in this order
 * for each stage:
 * <ul>
 *   <li>{@value #MOVE_TRIALS} move trials. The first {@value #FIXED_TRIALS} run on the wiring as
 *       imported (user fixed, so most are refused), then every user-fixed trace and via is
 *       unfixed. A trial draws a kind ({@code nextInt(10)} below 7: a pin of the component {@code
 *       nextInt(componentCount)}, else a via or trace of the item list, highest id first), the
 *       item, then {@code 3 + nextInt(4)} operations, each {@code nextInt(7)}: 0 and 1 a move by
 *       {@code nextInt(6001) - 3000} in x and y (from the session's current position), 2 a turn by
 *       {@code 1 + nextInt(3)} quarters, 3 a rotation by one of 45, -45, 30, 135, 12.5, 4 a
 *       mirror, 5 a reset of the rotation, 6 a move by {@code nextInt(60001) - 30000}. The trial
 *       ends with {@code nextBoolean()}: complete (cancel if the copies violate a clearance) or
 *       cancel. Trials with {@code k % 4 == 3} use a 500 by 500 component grid.
 *   <li>{@value #COPY_TRIALS} copy trials: {@code 1 + nextInt(3)} items (a pin, via or keepout),
 *       a move by {@code nextInt(8001) - 4000} in x and y and, one time in three, a change to
 *       the layer {@code nextInt(layerCount)}, an insert, and the same again.
 * </ul>
 *
 * <p>Lines: {@code move-start}, {@code move-op} and {@code move-end} per trial and {@code
 * copy-start} and {@code copy-insert}; the floating or inserted items as {@link #itemText} lines,
 * the components, the id counter and the FNV-1a 64 over all board items. {@code move-op-tree},
 * {@code move-tree} and {@code copy-tree} follow with the default search tree's fingerprint (as
 * {@code TreeOpsProbe} writes it): the temporary insertions of {@code ComponentItemTransform}
 * change the tree's shape, and the work limits of the router count its nodes.
 */
public class TransformProbe {
  static final long SEED = 20261016L;
  static final int MOVE_TRIALS = 12;
  static final int FIXED_TRIALS = 4;
  static final int COPY_TRIALS = 8;
  static final double[] ANGLES = {45.0, -45.0, 30.0, 135.0, 12.5};
  static final Map<String, Boolean> BOARDS =
      Map.of(
          "Issue026-J2_reference", true,
          "Issue508-DAC2020_bm05", true,
          "Issue955-rotated-pad-contacts", false);

  public static void main(String[] args) throws Exception {
    String board = Boards.board(args[0]);
    Boolean withSession = BOARDS.get(board);
    if (withSession == null) {
      System.exit(3);
    }
    Boards.Out out = new Boards.Out("transform", args[0]);
    try {
      long started = System.nanoTime();
      TransformProbe probe = new TransformProbe(out);
      probe.board = Boards.load(args[0]);
      probe.stage = "load";
      probe.script(false);
      if (withSession) {
        probe.board = Boards.load(args[0]);
        Boards.importSession(probe.board, args[1]);
        probe.stage = "ses";
        probe.script(true);
      }
      System.err.printf("transform: %.1f ms%n", (System.nanoTime() - started) / 1e6);
    } catch (Throwable t) {
      out.exception(t);
    }
    out.write(args[2]);
  }

  final Boards.Out out;
  RoutingBoard board;
  String stage;
  final Random random = new Random(SEED);

  TransformProbe(Boards.Out out) {
    this.out = out;
  }

  Boards.Line line(String phase, String name, int trial) {
    return new Boards.Line().str("stage", stage).str("phase", phase).num(name, trial);
  }

  long maxId() {
    return board.communication.idGenerator.maxGeneratedId();
  }

  List<Item> sortedItems() {
    List<Item> items = new ArrayList<>(board.getItems());
    items.sort((a, b) -> Integer.compare(b.getId(), a.getId()));
    return items;
  }

  /** Unfixes the user-fixed traces and vias, as the drag probe does. */
  void unfixWiring() {
    for (Item item : sortedItems()) {
      if ((item instanceof Trace || item instanceof Via)
          && item.getFixedState() == FixedState.USER_FIXED) {
        item.setFixedState(FixedState.UNFIXED);
      }
    }
  }

  static String join(Collection<Integer> numbers) {
    StringBuilder text = new StringBuilder();
    for (int n : numbers) {
      text.append(text.length() > 0 ? "," : "").append(n);
    }
    return text.toString();
  }

  static String ids(Collection<? extends Item> items) {
    StringBuilder text = new StringBuilder();
    for (Item item : items) {
      text.append(text.length() > 0 ? "," : "").append(item.getId());
    }
    return text.toString();
  }

  String texts(Collection<? extends Item> items) {
    StringBuilder text = new StringBuilder();
    for (Item item : items) {
      text.append(text.length() > 0 ? " | " : "").append(itemText(item).trim());
    }
    return text.toString();
  }

  String components() {
    StringBuilder text = new StringBuilder();
    for (Component c : board.components.getAll()) {
      text.append(text.length() > 0 ? ";" : "").append(c.name).append('|');
      if (c.isPlaced()) {
        FloatPoint p = c.getExactLocation();
        text.append(Boards.bits(p.x)).append(',').append(Boards.bits(p.y));
      } else {
        text.append('-');
      }
      text.append('|')
          .append(Boards.bits(c.getRotationInDegree()))
          .append('|')
          .append(c.placedOnFront() ? 'F' : 'B');
    }
    return text.toString();
  }

  long itemsHash() {
    long hash = Boards.FNV_OFFSET;
    for (Item item : sortedItems()) {
      hash = Boards.fnv(hash, itemText(item));
    }
    return hash;
  }

  static String point(Point p) {
    if (p instanceof IntPoint ip) {
      return ip.x + "," + ip.y;
    }
    FloatPoint f = p.toFloat();
    return "f" + Boards.bits(f.x) + "," + Boards.bits(f.y);
  }

  static String box(IntBox b) {
    return b.ll.x + "," + b.ll.y + "," + b.ur.x + "," + b.ur.y;
  }

  /** As {@code ItemOpsProbe.itemText}. */
  static String itemText(Item item) {
    StringBuilder text = new StringBuilder();
    text.append(item.getId()).append(' ').append(item.getClass().getSimpleName());
    text.append(" n");
    for (int i = 0; i < item.netCount(); i++) {
      text.append(i > 0 ? "," : "").append(item.getNetNumber(i));
    }
    text.append(" f").append(item.getFixedState().ordinal());
    text.append(" c").append(item.clearanceClassIndex());
    if (item instanceof DrillItem drill) {
      text.append(' ').append(point(drill.getCenter()));
      text.append(" l").append(drill.firstLayer()).append('-').append(drill.lastLayer());
      text.append(' ').append(box(drill.boundingBox()));
    } else if (item instanceof PolylineTrace trace) {
      text.append(" l").append(trace.getLayer()).append(" w").append(trace.getHalfWidth());
      Polyline polyline = trace.polyline();
      for (int i = 0; i < polyline.cornerCount(); i++) {
        text.append(' ').append(point(polyline.corner(i)));
      }
    } else if (item instanceof ObstacleArea area) {
      text.append(" l").append(area.getLayer()).append(' ').append(box(area.boundingBox()));
    } else {
      text.append(' ').append(box(item.boundingBox()));
    }
    text.append(" k").append(item.getComponentId());
    text.append('\n');
    return text.toString();
  }

  // ---------------------------------------------------------------------------------------------
  // The script
  // ---------------------------------------------------------------------------------------------

  void script(boolean withSession) {
    for (int k = 0; k < MOVE_TRIALS; k++) {
      if (k == FIXED_TRIALS) {
        unfixWiring();
      }
      moveTrial(k);
    }
    for (int k = 0; k < COPY_TRIALS; k++) {
      copyTrial(k);
    }
  }

  // ---------------------------------------------------------------------------------------------
  // Move: MoveItemState
  // ---------------------------------------------------------------------------------------------

  /** The state of {@code MoveItemState}. */
  final class Move {
    final TreeSet<Item> itemList = new TreeSet<>();
    final TreeSet<Component> componentList = new TreeSet<>();
    Component gridSnapComponent;
    final int gridH;
    final int gridV;
    IntPoint currentPosition;
    IntPoint previousPosition;
    Collection<ClearanceViolation> clearanceViolations = new LinkedList<>();
    final TreeSet<Integer> nets = new TreeSet<>();
    String refusal = "-";
    final TreeSet<Item> select = new TreeSet<>();

    Move(int gridH, int gridV) {
      this.gridH = gridH;
      this.gridV = gridV;
    }

    /** {@code getInstance}; false if refused (then {@link #refusal} and {@link #select}). */
    boolean start(Collection<Item> selection, FloatPoint location) {
      // extend itemList to full components
      TreeSet<Item> allItems = new TreeSet<>();
      TreeSet<Component> components = new TreeSet<>();
      Component gridSnap = null;
      for (Item currentItem : selection) {
        if (currentItem.getComponentId() > 0) {
          Component currentComponent = board.components.get(currentItem.getComponentId());
          if (currentComponent == null) {
            refusal = "inconsistent_component";
            return false;
          }
          if (gridSnap == null && (gridH > 0 || gridV > 0)) {
            gridSnap = currentComponent;
          }
          if (!components.contains(currentComponent)) {
            Collection<Item> componentItems = board.getComponentItems(currentComponent.id);
            for (Item currentComponentItem : componentItems) {
              components.add(currentComponent);
              allItems.add(currentComponentItem);
            }
          }
        } else {
          allItems.add(currentItem);
        }
      }
      boolean moveOk = true;
      TreeSet<Item> fixedItems = new TreeSet<>();
      TreeSet<Item> obstacleItems = new TreeSet<>();
      TreeSet<Item> addItems = new TreeSet<>();
      for (Item currentItem : allItems) {
        if (currentItem.isUserFixed()
            || (currentItem.getComponentId() > 0
                && board.components.get(currentItem.getComponentId()).positionFixed)) {
          moveOk = false;
          obstacleItems.add(currentItem);
          fixedItems.add(currentItem);
        } else if (currentItem.isConnected()) {
          boolean itemMovable = true;
          Collection<Item> contacts = currentItem.getConnectedSet(-1, true);
          {
            for (Item currentContact : contacts) {
              if (currentContact instanceof ConductionArea) {
                continue;
              }
              if (currentContact.isUserFixed()) {
                itemMovable = false;
                fixedItems.add(currentContact);
              } else if (currentContact.getComponentId() != 0) {
                Component currentComponent =
                    board.components.get(currentContact.getComponentId());
                if (!components.contains(currentComponent)) {
                  itemMovable = false;
                }
              }
              if (itemMovable) {
                addItems.add(currentContact);
              } else {
                obstacleItems.add(currentContact);
              }
            }
          }
          if (!itemMovable) {
            moveOk = false;
          }
        }
      }
      if (!moveOk) {
        if (!fixedItems.isEmpty()) {
          select.addAll(fixedItems);
          refusal = "please_unfix_selected_items_before_moving";
        } else {
          select.addAll(obstacleItems);
          refusal = "please_unroute_or_extend_selection_before_moving";
        }
        return false;
      }
      allItems.addAll(addItems);
      // Items the board keeps when detached (a plane's conduction area) are refused:
      // Java would insert them twice (DIVERGENCES, "Interactive editor").
      for (Item item : allItems) {
        if (item.getComponentId() <= 0 && item.isDeletionForbidden()) {
          refusal = "some_items_cannot_be_moved_because_they_are_fixed";
          return false;
        }
      }
      // the constructor
      componentList.addAll(components);
      gridSnapComponent = gridSnap;
      currentPosition = location.round();
      previousPosition = currentPosition;
      board.generateSnapshot();
      for (Item currentItem : allItems) {
        board.detachItemForMove(currentItem);
      }
      for (Item currentItem : allItems) {
        Item copiedItem = currentItem.copy(0);
        for (int i = 0; i < currentItem.netCount(); i++) {
          nets.add(currentItem.getNetNumber(i));
        }
        itemList.add(copiedItem);
      }
      return true;
    }

    void move(FloatPoint newPosition) {
      currentPosition = newPosition.round();
      if (!currentPosition.equals(previousPosition)) {
        Vector translateVector = currentPosition.differenceBy(previousPosition);
        if (gridSnapComponent != null) {
          translateVector = adjustToPlacementGrid(translateVector);
        }
        for (Component currentComponent : componentList) {
          board.components.move(currentComponent.id, translateVector);
        }
        clearanceViolations = new LinkedList<>();
        for (Item currentItem : itemList) {
          currentItem.translateBy(translateVector);
          clearanceViolations.addAll(currentItem.clearanceViolations());
        }
        previousPosition = currentPosition;
      }
    }

    Vector adjustToPlacementGrid(Vector vector) {
      Point newComponentLocation = gridSnapComponent.getLocation().translateBy(vector);
      IntPoint roundedComponentLocation =
          newComponentLocation.toFloat().roundToGrid(gridH, gridV);
      Vector adjustment = roundedComponentLocation.differenceBy(newComponentLocation);
      Vector result = vector.add(adjustment);
      currentPosition = previousPosition.translateBy(result).toFloat().round();
      return vector.add(adjustment);
    }

    /** {@code transformItems}; the status, "-" if it worked. */
    String transformItems(double angle, boolean mirror) {
      var transformed =
          ComponentItemTransform.apply(board, itemList, angle, mirror, currentPosition);
      if (transformed == null) {
        return "insertion_failed_because_of_obstacles";
      }
      itemList.clear();
      itemList.addAll(transformed);
      nets.clear();
      clearanceViolations = new LinkedList<>();
      for (Item item : itemList) {
        for (int i = 0; i < item.netCount(); i++) {
          nets.add(item.getNetNumber(i));
        }
        clearanceViolations.addAll(item.clearanceViolations());
      }
      return "-";
    }

    String turn90Degree(int factor) {
      if (factor == 0) {
        return "-";
      }
      return transformItems(factor * 90.0, false);
    }

    String rotate(double angleInDegree) {
      if (angleInDegree != 0) {
        return transformItems(angleInDegree, false);
      }
      return "-";
    }

    String changePlacementSide() {
      LayerStructure layerStructure = board.layerStructure;
      boolean placementSideChangable = true;
      for (Item currentItem : itemList) {
        if (currentItem instanceof Via via) {
          if (board.library.getMirroredViaPadstack(via.getPadstack()) == null) {
            placementSideChangable = false;
            break;
          }
        } else if (currentItem.firstLayer() == currentItem.lastLayer()) {
          int newLayerNo = board.getLayerCount() - currentItem.firstLayer() - 1;
          if (!layerStructure.layers[newLayerNo].isSignal) {
            placementSideChangable = false;
            break;
          }
        }
      }
      if (!placementSideChangable) {
        return "cannot_change_placement_side";
      }
      return transformItems(0, true);
    }

    String resetRotation() {
      Component componentToReset = null;
      for (Component currentComponent : componentList) {
        if (componentToReset == null) {
          componentToReset = currentComponent;
        } else if (componentToReset.getRotationInDegree()
            != currentComponent.getRotationInDegree()) {
          return "unable_to_reset_components_with_different_rotations";
        }
      }
      if (componentToReset == null) {
        return "-";
      }
      double rotation = componentToReset.getRotationInDegree();
      if (!board.components.getFlipStyleRotateFirst() || componentToReset.placedOnFront()) {
        rotation = 360 - rotation;
      }
      return rotate(rotation);
    }

    /** {@code complete}: the status, "-" if the copies were inserted. */
    String complete() {
      for (Item currentItem : itemList) {
        if (currentItem.clearanceViolationCount() > 0) {
          return "insertion_failed_because_of_obstacles";
        }
      }
      for (Item currentItem : itemList) {
        board.insertItem(currentItem);
      }
      return "-";
    }

    void cancel() {
      board.undo(null);
    }
  }

  List<Item> pinsOf(int componentId) {
    List<Item> pins = new ArrayList<>();
    for (Item item : sortedItems()) {
      if (item instanceof Pin && item.getComponentId() == componentId) {
        pins.add(item);
      }
    }
    return pins;
  }

  void moveTrial(int k) {
    int kind = random.nextInt(10);
    Item picked = null;
    if (kind < 7) {
      int count = board.components.count();
      if (count > 0) {
        Component component = board.components.get(1 + random.nextInt(count));
        List<Item> pins = pinsOf(component.id);
        if (!pins.isEmpty()) {
          picked = pins.get(random.nextInt(pins.size()));
        }
      }
    } else {
      List<Item> wiring = new ArrayList<>();
      for (Item item : sortedItems()) {
        if (item instanceof Via || item instanceof PolylineTrace) {
          wiring.add(item);
        }
      }
      if (!wiring.isEmpty()) {
        picked = wiring.get(random.nextInt(wiring.size()));
      }
    }
    if (picked == null) {
      out.line(line("move-start", "trial", k).str("result", "skip").end());
      return;
    }
    int grid = k % 4 == 3 ? 500 : 0;
    FloatPoint at =
        picked instanceof DrillItem drill
            ? drill.getCenter().toFloat()
            : picked.boundingBox().centreOfGravity();
    Move move = new Move(grid, grid);
    TreeSet<Item> selection = new TreeSet<>();
    selection.add(picked);
    boolean ok = move.start(selection, at);
    out.line(
        line("move-start", "trial", k)
            .num("grid", grid)
            .str("selection", ids(selection))
            .str("at", point(at.round()))
            .str("result", ok ? "ok" : "refused")
            .str("status", move.refusal)
            .str("select", ids(move.select))
            .str("floating", ids(move.itemList))
            .num("maxGeneratedId", maxId())
            .end());
    if (!ok) {
      return;
    }
    int operations = 3 + random.nextInt(4);
    for (int n = 0; n < operations; n++) {
      int type = random.nextInt(7);
      String op;
      String status = "-";
      switch (type) {
        case 0, 1, 6 -> {
          int span = type == 6 ? 60001 : 6001;
          int dx = random.nextInt(span) - span / 2;
          int dy = random.nextInt(span) - span / 2;
          op = "M " + dx + " " + dy;
          move.move(
              new FloatPoint(move.currentPosition.x + dx, move.currentPosition.y + dy));
        }
        case 2 -> {
          int factor = 1 + random.nextInt(3);
          op = "T " + factor;
          status = move.turn90Degree(factor);
        }
        case 3 -> {
          double angle = ANGLES[random.nextInt(ANGLES.length)];
          op = "R " + Boards.bits(angle);
          status = move.rotate(angle);
        }
        case 4 -> {
          op = "X";
          status = move.changePlacementSide();
        }
        default -> {
          op = "Z";
          status = move.resetRotation();
        }
      }
      out.line(
          line("move-op", "trial", k)
              .num("op", n)
              .str("what", op)
              .str("status", status)
              .num("violations", move.clearanceViolations.size())
              .str("nets", join(move.nets))
              .str("position", point(move.currentPosition))
              .str("items", texts(move.itemList))
              .str("components", components())
              .num("maxGeneratedId", maxId())
              .end());
      treeLine("move-op-tree", k);
    }
    String how;
    String status;
    if (random.nextBoolean()) {
      status = move.complete();
      if (status.equals("-")) {
        how = "complete";
      } else {
        move.cancel();
        how = "blocked-cancel";
      }
    } else {
      move.cancel();
      how = "cancel";
      status = "-";
    }
    out.line(
        line("move-end", "trial", k)
            .str("how", how)
            .str("status", status)
            .str("itemsHash", Boards.hex(itemsHash()))
            .num("items", board.getItems().size())
            .str("components", components())
            .num("maxGeneratedId", maxId())
            .end());
    treeLine("move-tree", k);
  }

  /** The default tree's shape after a trial's step: the temporary insertions leave their mark. */
  void treeLine(String phase, int trial) {
    try {
      out.line(hash(line(phase, "trial", trial), board.searchTreeManager.getDefaultTree()));
    } catch (Exception e) {
      throw new IllegalStateException(e);
    }
  }

  // ---------------------------------------------------------------------------------------------
  // Copy: CopyItemState
  // ---------------------------------------------------------------------------------------------

  /** The state of {@code CopyItemState}. */
  final class Copy {
    final Collection<Item> itemList = new LinkedList<>();
    IntPoint startPosition;
    IntPoint currentPosition;
    int currentLayer = 0;
    boolean layerChanged = false;
    IntPoint previousPosition;

    Copy(Collection<Item> items, FloatPoint location) {
      startPosition = location.round();
      currentPosition = startPosition;
      previousPosition = currentPosition;
      for (Item currentItem : items) {
        if (currentItem instanceof DrillItem) {
          Item newItem = currentItem.copy(0);
          itemList.add(newItem);
        } else if (currentItem instanceof ObstacleArea obs) {
          if (obs.getComponentId() > 0 && obs.name != null && obs.name.endsWith("_aux")) {
            continue;
          }
          Item newItem = currentItem.copy(0);
          itemList.add(newItem);
        }
      }
    }

    Padstack changePadstackLayers(
        Padstack oldPadstack, int newLayer, Map<Integer, Padstack> padstackPairs) {
      Padstack newPadstack;
      int oldLayer = oldPadstack.fromLayer();
      if (oldLayer == newLayer) {
        newPadstack = oldPadstack;
      } else if (padstackPairs.containsKey(oldPadstack.id)) {
        newPadstack = padstackPairs.get(oldPadstack.id);
      } else {
        ConvexShape[] newShapes = new ConvexShape[board.getLayerCount()];
        ConvexShape[][] newAuxShapes =
            oldPadstack.hasAuxiliaryShapes() ? new ConvexShape[board.getLayerCount()][] : null;
        int layerDiff = oldLayer - newLayer;
        for (int i = 0; i < newShapes.length; i++) {
          int oldLayerNo = i + layerDiff;
          if (oldLayerNo >= 0 && oldLayerNo < newShapes.length) {
            newShapes[i] = oldPadstack.getShape(oldLayerNo);
            if (newAuxShapes != null) {
              newAuxShapes[i] = oldPadstack.getAuxiliaryShapes(oldLayerNo);
            }
          }
        }
        if (newAuxShapes != null) {
          newPadstack = board.library.padstacks.add(newShapes, newAuxShapes);
          if (oldPadstack.hasNonConvexGeometry()) {
            newPadstack.hasNonConvexGeometry = true;
          }
        } else {
          newPadstack = board.library.padstacks.add(newShapes);
        }
        padstackPairs.put(oldPadstack.id, newPadstack);
      }
      return newPadstack;
    }

    void changePosition(FloatPoint newPosition) {
      currentPosition = newPosition.round();
      if (!currentPosition.equals(previousPosition)) {
        Vector translateVector = currentPosition.differenceBy(previousPosition);
        for (Item currentItem : itemList) {
          currentItem.translateBy(translateVector);
        }
        previousPosition = currentPosition;
      }
    }

    void changeLayerAction(int newLayer) {
      currentLayer = newLayer;
      layerChanged = true;
    }

    /** {@code insert}; the ids of the inserted items, and whether all were. */
    boolean insert(List<Item> inserted) {
      Map<Integer, Padstack> padstackPairs = new TreeMap<>();
      if (layerChanged) {
        for (Item currentObject : itemList) {
          if (currentObject instanceof Via currentVia) {
            Padstack newPadstack =
                changePadstackLayers(currentVia.getPadstack(), currentLayer, padstackPairs);
            currentVia.setPadstack(newPadstack);
          }
        }
      }
      Map<Integer, Integer> cmpNoPairs = new TreeMap<>();
      Vector translateVector = currentPosition.differenceBy(startPosition);
      for (Item currentItem : itemList) {
        int currentCmpNo = currentItem.getComponentId();
        if (currentCmpNo > 0) {
          int newCmpNo;
          Integer currentKey = currentCmpNo;
          if (cmpNoPairs.containsKey(currentKey)) {
            newCmpNo = cmpNoPairs.get(currentKey);
          } else {
            Component oldComponent = board.components.get(currentCmpNo);
            if (oldComponent == null) {
              continue;
            }
            Package newPackage;
            if (layerChanged) {
              Package.Pin[] newPinArr = new Package.Pin[oldComponent.getPackage().pinCount()];
              for (int i = 0; i < newPinArr.length; i++) {
                Package.Pin oldPin = oldComponent.getPackage().getPin(i);
                Padstack oldPadstack = board.library.padstacks.get(oldPin.padstackId);
                if (oldPadstack == null) {
                  return false;
                }
                Padstack newPadstack =
                    changePadstackLayers(oldPadstack, currentLayer, padstackPairs);
                newPinArr[i] =
                    new Package.Pin(
                        oldPin.name,
                        newPadstack.id,
                        oldPin.getExactRelativeLocation(),
                        oldPin.rotationInDegree);
              }
              newPackage = board.library.packages.add(newPinArr);
            } else {
              newPackage = oldComponent.getPackage();
            }
            Component newComponent =
                board.components.copy(oldComponent, translateVector, newPackage);
            newCmpNo = newComponent.id;
            cmpNoPairs.put(currentCmpNo, newCmpNo);
          }
          currentItem.assignComponentId(newCmpNo);
        }
      }
      boolean allItemsInserted = true;
      boolean firstTime = true;
      for (Item currentItem : itemList) {
        if (currentItem.board != null && currentItem.clearanceViolationCount() == 0) {
          if (firstTime) {
            board.generateSnapshot();
            firstTime = false;
          }
          Item copiedItem = currentItem.copy(0);
          board.insertItem(copiedItem);
          inserted.add(copiedItem);
          if (copiedItem instanceof Pin copiedPin && copiedPin.getPadstack().hasAuxiliaryShapes()) {
            Padstack padstack = copiedPin.getPadstack();
            Component component = board.components.get(copiedPin.getComponentId());
            boolean onFront = component == null || component.placedOnFront();
            boolean absolute = padstack.placedAbsolute;
            int layerCount = padstack.boardLayerCount();
            for (int boardLayer = copiedPin.firstLayer();
                boardLayer <= copiedPin.lastLayer();
                boardLayer++) {
              int padstackLayer = (onFront || absolute) ? boardLayer : layerCount - boardLayer - 1;
              ConvexShape[] auxShapes = padstack.getAuxiliaryShapes(padstackLayer);
              if (auxShapes != null) {
                for (ConvexShape auxShape : auxShapes) {
                  if (auxShape != null) {
                    Shape boardShape = copiedPin.transformToBoard(auxShape);
                    Area area = null;
                    if (boardShape instanceof PolylineShape polyShape) {
                      area = new PolylineArea(polyShape, new PolylineShape[0]);
                    } else if (boardShape != null) {
                      area = boardShape;
                    }
                    if (area != null && !area.isEmpty()) {
                      ObstacleArea aux =
                          board.insertObstacle(
                              area,
                              boardLayer,
                              copiedPin.netNumbers,
                              copiedPin.clearanceClassIndex(),
                              copiedPin.getComponentId(),
                              padstack.name + "_aux",
                              copiedPin.getFixedState());
                      inserted.add(aux);
                    }
                  }
                }
              }
            }
          }
        } else {
          allItemsInserted = false;
        }
      }
      startPosition = currentPosition;
      layerChanged = false;
      return allItemsInserted;
    }
  }

  void copyTrial(int k) {
    List<Item> pins = new ArrayList<>();
    List<Item> vias = new ArrayList<>();
    List<Item> keepouts = new ArrayList<>();
    for (Item item : sortedItems()) {
      if (item instanceof Pin) {
        pins.add(item);
      } else if (item instanceof Via) {
        vias.add(item);
      } else if (item instanceof ObstacleArea && !(item instanceof ConductionArea)) {
        keepouts.add(item);
      }
    }
    TreeSet<Item> selection = new TreeSet<>();
    int picks = 1 + random.nextInt(3);
    for (int i = 0; i < picks; i++) {
      int type = random.nextInt(10);
      List<Item> from = type < 6 ? pins : type < 8 ? vias : keepouts;
      if (!from.isEmpty()) {
        selection.add(from.get(random.nextInt(from.size())));
      }
    }
    if (selection.isEmpty()) {
      out.line(line("copy-start", "trial", k).str("result", "skip").end());
      return;
    }
    Item first = selection.first();
    FloatPoint at =
        first instanceof DrillItem drill
            ? drill.getCenter().toFloat()
            : first.boundingBox().centreOfGravity();
    Copy copy = new Copy(selection, at);
    out.line(
        line("copy-start", "trial", k)
            .str("selection", ids(selection))
            .str("at", point(at.round()))
            .str("preview", ids(copy.itemList))
            .num("maxGeneratedId", maxId())
            .end());
    for (int n = 0; n < 2; n++) {
      int dx = random.nextInt(8001) - 4000;
      int dy = random.nextInt(8001) - 4000;
      copy.changePosition(
          new FloatPoint(copy.currentPosition.x + dx, copy.currentPosition.y + dy));
      int layer = -1;
      if (random.nextInt(3) == 0) {
        layer = random.nextInt(board.getLayerCount());
        copy.changeLayerAction(layer);
      }
      List<Item> inserted = new ArrayList<>();
      boolean all = copy.insert(inserted);
      out.line(
          line("copy-insert", "trial", k)
              .num("insert", n)
              .str("offset", dx + "," + dy)
              .num("layer", layer)
              .bool("all", all)
              .str("inserted", ids(inserted))
              .str("items", texts(inserted))
              .num("padstacks", board.library.padstacks.count())
              .num("packages", board.library.packages.count())
              .str("components", components())
              .str("itemsHash", Boards.hex(itemsHash()))
              .num("maxGeneratedId", maxId())
              .end());
      treeLine("copy-tree", k);
    }
  }

  // ---------------------------------------------------------------------------------------------
  // The default tree
  // ---------------------------------------------------------------------------------------------

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

  /** One line for {@code tree}, as {@code TreeOpsProbe.hash} writes it, after the trial's keys. */
  static String hash(Boards.Line head, ShapeSearchTree tree) throws Exception {
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
    return head
            .str("tree", tree.key)
            .num("nodes", nodes)
            .num("leaves", leaves)
            .num("maxDepth", maxDepth)
            .str("shapeHash", Boards.hex(shapeHash))
            .str("contentDigest", Boards.hex(contentDigest))
            .end();
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
