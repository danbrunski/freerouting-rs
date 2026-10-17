package app.freerouting.autoroute.pipeline;

import app.freerouting.Freerouting;
import app.freerouting.autoroute.ItemRouteResult;
import app.freerouting.board.actions.ItemIdGenerator;
import app.freerouting.board.facade.RoutingBoard;
import app.freerouting.board.model.items.Item;
import app.freerouting.board.model.items.Trace;
import app.freerouting.board.model.items.Via;
import app.freerouting.core.RoutingJob;
import app.freerouting.core.StoppableThread;
import app.freerouting.io.specctra.RulesReader;
import app.freerouting.io.specctra.SesReader;
import app.freerouting.management.BoardLoader;
import app.freerouting.management.HeadlessBoardManager;
import app.freerouting.settings.GlobalSettings;
import app.freerouting.settings.SettingsMerger;
import app.freerouting.settings.sources.ApiSettings;
import app.freerouting.settings.sources.DefaultSettings;
import app.freerouting.settings.sources.DsnFileSettings;
import app.freerouting.settings.sources.RulesFileSettings;
import java.io.ByteArrayInputStream;
import java.io.File;
import java.io.FileInputStream;
import java.io.InputStream;
import java.lang.reflect.Field;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.List;
import java.util.Set;
import java.util.UUID;

/**
 * Probe optimize-candidates (O06.2, O06.3): the batch optimizer's candidate order and its
 * one-candidate reroute.
 *
 * <pre>OptimizeCandidatesProbe &lt;dsn&gt; &lt;ses&gt; &lt;out.jsonl&gt;</pre>
 *
 * <p>The board loads as the routing job does ({@code AutorouteProbe.Loaded}: the default
 * settings, the DSN's, an adjacent rules file), the session is imported with {@code SesReader},
 * and every trace and via is unfixed ({@code item.unfix()}, as {@code AutorouteProbe}'s
 * last-mile board does), because {@code SesReader} imports them user fixed and a user-fixed item
 * is never a candidate. Then:
 *
 * <ul>
 *   <li>{@code stage order}: the SEQUENTIAL candidate id list of pass 1, read with {@code
 *       BatchOptimizer.ReadSortedRouteItems.next} until it returns null;
 *   <li>{@code stage items} (the {@link #ITEM_BOARDS} only): the first {@link #ITEMS}
 *       candidates, each rerouted by {@code optRouteItemOnBoard} with pass 1's parameters
 *       (preferred directions, increased ripup costs, the baseline's weighted length) on its
 *       own {@code deepCopy()} of the baseline after {@code generateSnapshot()}, and with its own
 *       no-op stop thread (the inner autorouter's {@code max_items} stop requests a stop on the
 *       job thread, which would leak into the next candidate). One line per candidate with
 *       every {@code ItemRouteResult} field (doubles and floats as raw bits).
 * </ul>
 *
 * Exits 3 on boards outside {@link #ORDER_BOARDS} and when the session is {@code -}.
 */
public final class OptimizeCandidatesProbe {

  /** O06.7's six boards plus four fully routed route references (O06.2). */
  static final Set<String> ORDER_BOARDS =
      Set.of(
          "Issue508-DAC2020_bm08",
          "Issue026-J2_reference",
          "Issue508-DAC2020_bm05",
          "Issue954-circle",
          "Issue508-SMD-routing-issue-demo",
          "Issue269-min_fr_test/min_fr_test_no_quotes",
          "Issue143-rpi_splitter",
          "Issue433-my-board",
          "Issue413-test",
          "Issue954-polygon64");

  /** O06.7's six boards (O06.3). */
  static final Set<String> ITEM_BOARDS =
      Set.of(
          "Issue508-DAC2020_bm08",
          "Issue026-J2_reference",
          "Issue508-DAC2020_bm05",
          "Issue954-circle",
          "Issue508-SMD-routing-issue-demo",
          "Issue269-min_fr_test/min_fr_test_no_quotes");

  static final int ITEMS = 20;

  public static void main(String[] args) throws Exception {
    String board = Boards.board(args[0]);
    if (args[1].equals("-") || !ORDER_BOARDS.contains(board)) {
      System.exit(3);
    }
    Boards.Out out = new Boards.Out("optimize-candidates", args[0]);
    try {
      RoutingJob job = load(args[0]);
      try (InputStream in = new FileInputStream(args[1])) {
        SesReader.read(in, job.board);
      }
      for (Item item : job.board.getItems()) {
        if (item instanceof Trace || item instanceof Via) {
          item.unfix();
        }
      }
      job.thread = new NoOpThread();
      BatchOptimizer optimizer = BatchOptimizer.create(job);
      BatchOptimizer.ReadSortedRouteItems sorted = optimizer.new ReadSortedRouteItems();
      List<Integer> ids = new ArrayList<>();
      for (Item item = sorted.next(); item != null; item = sorted.next()) {
        ids.add(item.getId());
      }
      StringBuilder list = new StringBuilder("[");
      for (int i = 0; i < ids.size(); i++) {
        list.append(i == 0 ? "" : ",").append(ids.get(i));
      }
      list.append(']');
      out.line(
          new Boards.Line()
              .str("stage", "order")
              .str("ses", args[1].replace('\\', '/'))
              .num("count", ids.size())
              .raw("ids", list.toString())
              .end());
      if (ITEM_BOARDS.contains(board)) {
        items(out, job, ids);
      }
    } catch (Throwable t) {
      out.exception(t);
    }
    out.write(args[2]);
  }

  /** The first {@link #ITEMS} candidates of pass 1, each on a fresh copy of the baseline. */
  static void items(Boards.Out out, RoutingJob job, List<Integer> ids) throws Exception {
    RoutingBoard baseline = job.board;
    double baselineLength = baseline.getStatistics().traces.totalWeightedLength;
    out.line(
        new Boards.Line()
            .str("stage", "items")
            .dbl("baseline", baselineLength)
            .num("count", Math.min(ITEMS, ids.size()))
            .end());
    for (int i = 0; i < Math.min(ITEMS, ids.size()); i++) {
      RoutingBoard copy = baseline.deepCopy();
      copy.generateSnapshot();
      Item item = copy.getItem(ids.get(i));
      job.thread = new NoOpThread();
      ItemRouteResult r =
          BatchOptimizer.optRouteItemOnBoard(
              job, copy, item, baselineLength, true, true, job.thread, null);
      out.line(
          new Boards.Line()
              .num("item", (Integer) field(r, "itemId"))
              .bool("improved", r.improved())
              .num("viasBefore", (Integer) field(r, "viaCountBefore"))
              .num("viasAfter", (Integer) field(r, "viaCountAfter"))
              .dbl("lengthBefore", (Double) field(r, "traceLengthBefore"))
              .dbl("lengthAfter", (Double) field(r, "traceLengthAfter"))
              .num("incompleteBefore", (Integer) field(r, "incompleteCountBefore"))
              .num("incompleteAfter", (Integer) field(r, "incompleteCountAfter"))
              .str("percent", Integer.toHexString(Float.floatToRawIntBits(r.improvementPercentage())))
              .str("hash", copy.getHash())
              .end());
    }
  }

  static Object field(ItemRouteResult r, String name) throws Exception {
    Field f = ItemRouteResult.class.getDeclaredField(name);
    f.setAccessible(true);
    return f.get(r);
  }

  /** A stop thread nothing stops ({@code BatchOptimizerDeterminismTest.NoOpStoppableThread}). */
  static final class NoOpThread extends StoppableThread {
    @Override
    protected void threadAction() {}
  }

  /** The routing job's load, as {@code AutorouteProbe.Loaded.load}. */
  static RoutingJob load(String dsn) throws Exception {
    Freerouting.globalSettings = new GlobalSettings();
    GlobalSettings gs = Freerouting.globalSettings;
    gs.settingsMergerProtype = new SettingsMerger(new DefaultSettings());
    RoutingJob job = new RoutingJob(new UUID(0L, 1L));
    job.setInput(new File(dsn));
    SettingsMerger first = gs.settingsMergerProtype.clone();
    first.addOrReplaceSources(new DsnFileSettings(job.input.getData(), job.input.getFilename()));
    job.routerSettings = first.merge();
    job.drcSettings = gs.drcSettings.clone();
    HeadlessBoardManager manager = new HeadlessBoardManager(job);
    manager.loadFromSpecctraDsn(job.input.getData(), null, new ItemIdGenerator());
    job.board = manager.getRoutingBoard();
    if (job.board == null) {
      throw new IllegalStateException("the board did not load");
    }
    job.board.awaitPostLoad();
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
      RulesReader.read(
          new ByteArrayInputStream(rules),
          job.name != null ? job.name : "board",
          job.board,
          job.routerSettings);
    }
    job.routerSettings.applyBoardSpecificOptimizations(job.board);
    job.routerSettings.applyNetClassExclusions(job.board);
    return job;
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
