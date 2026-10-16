package app.freerouting.board.optimize;

import app.freerouting.Freerouting;
import app.freerouting.autoroute.maze.AutorouteControl.ExpansionCostFactor;
import app.freerouting.board.facade.RoutingBoard;
import app.freerouting.board.model.items.Item;
import app.freerouting.board.model.items.Via;
import app.freerouting.board.trace.PolylineTrace;
import app.freerouting.core.RoutingJob;
import app.freerouting.datastructures.WorkMeter;
import app.freerouting.geometry.planar.FloatPoint;
import app.freerouting.geometry.planar.IntPoint;
import app.freerouting.geometry.planar.Point;
import app.freerouting.geometry.planar.Polyline;
import app.freerouting.io.specctra.SesReader;
import app.freerouting.management.BoardLoader;
import app.freerouting.settings.GlobalSettings;
import app.freerouting.settings.RouterSettings;
import app.freerouting.settings.SettingsMerger;
import app.freerouting.settings.sources.DefaultSettings;
import java.io.FileInputStream;
import java.io.InputStream;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.List;
import java.util.Set;
import java.util.UUID;

/**
 * Board probe {@code opt-vias} (O05.5): Java's via optimizer ({@code ViaOptimizer}) and the via
 * branch of {@code TraceTightener.optChangedArea} on the boards of probe {@code pull-tight}, 45
 * degree, with the trace costs the batch autorouter uses: {@code new SettingsMerger(new
 * DefaultSettings()).merge()}, {@code applyBoardSpecificOptimizations(board)}, {@code
 * getTraceCosts()} (the batch autorouter keeps preferred directions, {@code
 * BatchAutorouter.java:146-147}). Every trace and via is {@code unfix()}ed first, because {@code
 * SesReader} imports them {@code USER_FIXED}. No work limit anywhere.
 *
 * <ul>
 *   <li>{@code costs}: the cost array, one line per layer (horizontal and vertical as raw bits).
 *   <li>{@code vias-each}: with the changed area marked, {@code optViaLocation(board, via, costs,
 *       500, 10)} for every via of a snapshot, highest id first, that is still on the board; per
 *       via the result, the via's centre after it and {@code maxGeneratedId}; then the canonical
 *       item dump.
 *   <li>{@code board-optvias}: on a fresh board {@code startMarkingChangedArea();
 *       markAllChangedArea(); optChangedArea(new int[0], null, 500, costs, null, 0)}, then the
 *       dump.
 * </ul>
 *
 * <pre>
 * tools/java-reference/probe.sh &lt;pin&gt; tools/java-reference/probes/board/OptViasProbe.java \
 *     board.dsn board.ses out.jsonl
 * </pre>
 */
public class OptViasProbe {
  static final int ACCURACY = 500;
  static final int DEPTH = 10;
  // the board set of probe pull-tight (PullTightProbe.BOARD_SET): 41 boards
  static final Set<String> BOARD_SET = Set.of(
      "empty_board",
      "scoring-perfect-two-pin",
      "Issue756-minimal-ok",
      "Issue757-minimal-soe-ok",
      "Issue954-circle",
      "Issue756-minimal-hang",
      "Issue757-minimal-soe",
      "scoring-empty-net",
      "scoring-zero-length",
      "scoring-mixed-layer",
      "Issue508-SMD-routing-issue-demo",
      "Issue269-min_fr_test/min_fr_test_no_quotes",
      "Issue269-min_fr_test/min_fr_test",
      "Issue368-CorneyIslandWireless_input_design",
      "Issue143-rpi_splitter",
      "Issue270-non-ansi_bracket",
      "Issue143-rpi_splitter_mod",
      "PCBench-retroreflectors-CONGFLOCK",
      "Issue954-invalid-polygon64",
      "Issue954-polygon64",
      "Issue368-CorneyIslandWireless/corney_island_wireless",
      "Issue069-TestSensel/TestSensel-KiCad6",
      "Issue508-DAC2020/DAC2020_bm08/DAC2020_bm08.unrouted",
      "Issue508-DAC2020/DAC2020_bm08/FRv1.9.0/DAC2020_bm08.unrouted",
      "Issue508-DAC2020/DAC2020_bm08/FRv2.1.0/DAC2020_bm08.unrouted",
      "Issue508-DAC2020_bm08",
      "Issue069-TestSensel/TestSensel",
      "Issue110-testProjectFromFreeroutingBugTest",
      "Issue026-J2_reference",
      "Issue433-my-board",
      "Issue413-test",
      "Issue110-testProjectFromFreeroutingBugTest01",
      "Issue508-DAC2020/DAC2020_bm07/DAC2020_bm07.unrouted",
      "Issue508-DAC2020/DAC2020_bm07/FRv1.9.0/DAC2020_bm07.unrouted",
      "Issue508-DAC2020/DAC2020_bm07/FRv2.1.0/DAC2020_bm07.unrouted",
      "Issue508-DAC2020_bm07",
      "Issue957-smoke-ldo-led",
      "Issue508-DAC2020/DAC2020_bm05/DAC2020_bm05.unrouted",
      "Issue508-DAC2020/DAC2020_bm05/FRv1.9.0/DAC2020_bm05.unrouted",
      "Issue508-DAC2020/DAC2020_bm05/FRv2.1.0/DAC2020_bm05.unrouted",
      "Issue508-DAC2020_bm05");

  public static void main(String[] args) throws Exception {
    String board = Boards.board(args[0]);
    if (args[1].equals("-") || !BOARD_SET.contains(board)) {
      System.exit(3);
    }
    Boards.Out out = new Boards.Out("opt-vias", args[0]);
    try {
      ExpansionCostFactor[] costs = traceCosts(unfixed(args));
      out.line(new Boards.Line().str("stage", "costs").num("layers", costs.length).end());
      for (int i = 0; i < costs.length; i++) {
        out.line(
            new Boards.Line()
                .num("layer", i)
                .dbl("horizontal", costs[i].horizontal())
                .dbl("vertical", costs[i].vertical())
                .end());
      }
      viasEach(out, args, costs);
      boardMode(out, args, costs);
    } catch (Throwable t) {
      out.exception(t);
    }
    out.write(args[2]);
  }

  /** The batch autorouter's trace costs for {@code board} with default settings. */
  static ExpansionCostFactor[] traceCosts(RoutingBoard board) {
    RouterSettings settings = new SettingsMerger(new DefaultSettings()).merge();
    settings.applyBoardSpecificOptimizations(board);
    return settings.getTraceCosts();
  }

  /** The board with the session imported and every trace and via unfixed. */
  static RoutingBoard unfixed(String[] args) throws Exception {
    RoutingBoard board = Boards.load(args[0]);
    Boards.importSession(board, args[1]);
    for (Item item : board.getItems()) {
      if (item instanceof PolylineTrace || item instanceof Via) {
        item.unfix();
      }
    }
    return board;
  }

  static void viasEach(Boards.Out out, String[] args, ExpansionCostFactor[] costs)
      throws Exception {
    RoutingBoard board = unfixed(args);
    out.line(new Boards.Line().str("stage", "vias-each").end());
    List<Via> vias = new ArrayList<>();
    for (Item item : board.getItems()) {
      if (item instanceof Via via) {
        vias.add(via);
      }
    }
    vias.sort((a, b) -> Integer.compare(b.getId(), a.getId()));
    board.startMarkingChangedArea();
    int moved = 0;
    long start = System.nanoTime();
    for (Via via : vias) {
      if (!via.isOnTheBoard()) {
        continue;
      }
      boolean result = ViaOptimizer.optViaLocation(board, via, costs, ACCURACY, DEPTH);
      moved += result ? 1 : 0;
      out.line(
          new Boards.Line()
              .num("via", via.getId())
              .bool("result", result)
              .str("center", point(via.getCenter()))
              .num("maxGeneratedId", board.communication.idGenerator.maxGeneratedId())
              .end());
    }
    System.err.println(
        "TIMING " + Boards.board(args[0]) + " vias-each ms=" + (System.nanoTime() - start) / 1_000_000
            + " vias=" + vias.size() + " moved=" + moved);
    dump(out, board);
  }

  static void boardMode(Boards.Out out, String[] args, ExpansionCostFactor[] costs)
      throws Exception {
    RoutingBoard board = unfixed(args);
    out.line(new Boards.Line().str("stage", "board-optvias").end());
    long startUnits = WorkMeter.current().units();
    long start = System.nanoTime();
    board.startMarkingChangedArea();
    board.markAllChangedArea();
    board.optChangedArea(new int[0], null, ACCURACY, costs, null, 0);
    System.err.println(
        "TIMING " + Boards.board(args[0]) + " board-optvias ms="
            + (System.nanoTime() - start) / 1_000_000
            + " units=" + (WorkMeter.current().units() - startUnits));
    dump(out, board);
  }

  // ---- the canonical item dump (tests/support/item_dump.rs) ----

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
