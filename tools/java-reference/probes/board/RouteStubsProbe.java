import app.freerouting.Freerouting;
import app.freerouting.board.facade.RoutingBoard;
import app.freerouting.board.model.items.Item;
import app.freerouting.board.model.items.Trace;
import app.freerouting.board.model.items.Via;
import app.freerouting.board.state.CoordinateTransform;
import app.freerouting.core.RoutingJob;
import app.freerouting.datastructures.Signum;
import app.freerouting.geometry.planar.FloatPoint;
import app.freerouting.io.specctra.SesReader;
import app.freerouting.management.BoardLoader;
import app.freerouting.rules.Net;
import app.freerouting.settings.GlobalSettings;
import java.io.FileInputStream;
import java.io.InputStream;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.Collection;
import java.util.Iterator;
import java.util.Set;
import java.util.SortedSet;
import java.util.TreeSet;
import java.util.UUID;

/**
 * Board probe {@code route-stubs} (B08.3): the route stubs list of {@code
 * gui/windows/routing/WindowRouteStubs.fillList} (pin {@code a773a70ec}, lines 36-100) with its
 * {@code RouteStubInfo.compareTo} (lines 156-169). The window needs a GUI, so this probe is a
 * stand-in: {@link #stubs} is a copy of {@code fillList} and {@code RouteStubInfo} as written,
 * with the Swing and text parts removed, and the user coordinates taken from the same {@code
 * CoordinateTransform} that {@code GuiBoardManager} builds. One line {@code stage:"stubs"} with
 * the count, then one line {@code stage:"stub"} per list entry in list order; then the same
 * with the suffix {@code -after-removal} after every 7th trace was removed. Runs on the boards
 * in {@link #BOARDS}, with the session imported, and exits 3 on the others.
 *
 * <pre>
 * tools/java-reference/probe.sh &lt;pin&gt; tools/java-reference/probes/board/RouteStubsProbe.java \
 *     board.dsn board.ses out.jsonl
 * </pre>
 */
public class RouteStubsProbe {
  static final Set<String> BOARDS =
      Set.of("Issue026-J2_reference", "Issue508-DAC2020_bm05", "Issue690-ecc83");

  public static void main(String[] args) throws Exception {
    if (!BOARDS.contains(Boards.board(args[0])) || args[1].equals("-")) {
      System.exit(3);
    }
    Boards.Out out = new Boards.Out("route-stubs", args[0]);
    try {
      RoutingBoard board = Boards.load(args[0]);
      Boards.importSession(board, args[1]);
      dump(out, "", board);
      // Every 7th trace in item-list order (highest id first) is removed, which leaves stubs
      // on boards the router completed.
      java.util.List<Trace> traces = new java.util.ArrayList<>(board.getTraces());
      for (int i = 0; i < traces.size(); i += 7) {
        traces.get(i).unfix();
        board.removeItem(traces.get(i));
      }
      dump(out, "-after-removal", board);
    } catch (Throwable t) {
      out.exception(t);
    }
    out.write(args[2]);
  }

  static void dump(Boards.Out out, String suffix, RoutingBoard board) {
    SortedSet<RouteStubInfo> stubs = stubs(board);
    out.line(new Boards.Line().str("stage", "stubs" + suffix).num("count", stubs.size()).end());
    for (RouteStubInfo info : stubs) {
      out.line(
          new Boards.Line()
              .str("stage", "stub" + suffix)
              .num("item", info.stubItem.getId())
              .str("type", info.stubItem instanceof Trace ? "trace" : "via")
              .str("net", info.net.name)
              .dbl("x", info.location.x)
              .dbl("y", info.location.y)
              .dbl("boardX", info.boardLocation.x)
              .dbl("boardY", info.boardLocation.y)
              .num("layer", info.layerIndex)
              .end());
    }
  }

  /** {@code WindowRouteStubs.fillList}, copied; the list is the returned set. */
  static SortedSet<RouteStubInfo> stubs(RoutingBoard routingBoard) {
    double unitFactor = routingBoard.communication.coordinateTransform.boardToDsn(1);
    CoordinateTransform coordinateTransform =
        new CoordinateTransform(
            1, routingBoard.communication.unit, unitFactor, routingBoard.communication.unit);
    SortedSet<RouteStubInfo> routeStubInfoSet = new TreeSet<>();

    Collection<Item> boardItems = routingBoard.getItems();
    for (Item currentItem : boardItems) {
      if (!(currentItem instanceof Trace || currentItem instanceof Via)) {
        continue;
      }
      if (currentItem.netCount() != 1) {
        continue;
      }

      FloatPoint stubLocation;
      int stubLayer;
      if (currentItem instanceof Via via) {
        Collection<Item> contactList = currentItem.getAllContacts();
        if (contactList.isEmpty()) {
          stubLayer = currentItem.firstLayer();
        } else {
          Iterator<Item> it = contactList.iterator();
          Item currentContactItem = it.next();
          int firstContactFirstLayer = currentContactItem.firstLayer();
          int firstContactLastLayer = currentContactItem.lastLayer();
          boolean allContactsOnOneLayer = true;
          while (it.hasNext()) {
            currentContactItem = it.next();
            if (currentContactItem.firstLayer() != firstContactFirstLayer
                || currentContactItem.lastLayer() != firstContactLastLayer) {
              allContactsOnOneLayer = false;
              break;
            }
          }
          if (!allContactsOnOneLayer) {
            continue;
          }
          if (currentItem.firstLayer() >= firstContactFirstLayer
              && currentItem.lastLayer() <= firstContactFirstLayer) {
            stubLayer = firstContactFirstLayer;
          } else {
            stubLayer = firstContactLastLayer;
          }
        }
        stubLocation = via.getCenter().toFloat();
      } else {
        Trace currentTrace = (Trace) currentItem;
        if (currentTrace.getStartContacts().isEmpty()) {
          stubLocation = currentTrace.firstCorner().toFloat();
        } else if (currentTrace.getEndContacts().isEmpty()) {
          stubLocation = currentTrace.lastCorner().toFloat();
        } else {
          continue;
        }
        stubLayer = currentTrace.getLayer();
      }
      RouteStubInfo currentRouteStubInfo =
          new RouteStubInfo(routingBoard, coordinateTransform, currentItem, stubLocation, stubLayer);
      routeStubInfoSet.add(currentRouteStubInfo);
    }
    return routeStubInfoSet;
  }

  /** {@code WindowRouteStubs.RouteStubInfo}, copied without {@code toString}. */
  static final class RouteStubInfo implements Comparable<RouteStubInfo> {
    final Item stubItem;
    final Net net;
    final FloatPoint boardLocation;
    final FloatPoint location;
    final int layerIndex;

    RouteStubInfo(
        RoutingBoard board,
        CoordinateTransform coordinateTransform,
        Item stub,
        FloatPoint location,
        int layerIndex) {
      this.stubItem = stub;
      this.boardLocation = location;
      this.location = coordinateTransform.boardToUser(location);
      this.layerIndex = layerIndex;
      int netNumber = stub.getNetNumber(0);
      this.net = board.rules.nets.get(netNumber);
    }

    @Override
    public int compareTo(RouteStubInfo other) {
      int result = this.net.name.compareTo(other.net.name);
      if (result == 0) {
        result = Signum.asInt(this.location.x - other.location.x);
      }
      if (result == 0) {
        result = Signum.asInt(this.location.y - other.location.y);
      }
      if (result == 0) {
        result = this.layerIndex - other.layerIndex;
      }
      return result;
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
