package reference;

import app.freerouting.Freerouting;
import app.freerouting.board.actions.ItemIdGenerator;
import app.freerouting.board.facade.BasicBoard;
import app.freerouting.core.RoutingJob;
import app.freerouting.core.RoutingJobState;
import app.freerouting.core.Session;
import app.freerouting.core.scoring.BoardStatistics;
import app.freerouting.io.BoardReadResult;
import app.freerouting.io.specctra.DsnReader;
import app.freerouting.management.jobs.RoutingJobScheduler;
import app.freerouting.management.sessions.SessionManager;
import app.freerouting.settings.GlobalSettings;
import app.freerouting.settings.RouterSettings;
import app.freerouting.settings.SettingsMerger;
import app.freerouting.settings.SettingsSource;
import app.freerouting.settings.sources.DefaultSettings;
import app.freerouting.settings.sources.DsnFileSettings;
import com.google.gson.Gson;
import com.google.gson.GsonBuilder;
import com.google.gson.JsonElement;
import com.google.gson.JsonObject;
import com.google.gson.JsonParser;
import java.io.FileInputStream;
import java.io.IOException;
import java.io.InputStream;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.List;
import java.util.UUID;
import java.util.regex.Matcher;
import java.util.regex.Pattern;
import java.util.stream.Stream;
import app.freerouting.board.facade.RoutingBoard;
import app.freerouting.board.model.structure.BoardOutline;
import app.freerouting.core.library.Padstack;
import app.freerouting.drc.DesignRulesChecker;
import app.freerouting.io.FileFormat;
import app.freerouting.io.specctra.DsnWriter;
import app.freerouting.io.specctra.RulesReader;
import app.freerouting.io.specctra.RulesWriter;
import app.freerouting.io.specctra.SesImportSummary;
import app.freerouting.io.specctra.SesReader;
import app.freerouting.io.specctra.SesWriter;
import app.freerouting.logger.FRLogger;
import app.freerouting.logger.LogEntry;
import app.freerouting.management.HeadlessBoardManager;
import app.freerouting.rules.BoardRules;
import app.freerouting.rules.ClearanceMatrix;
import app.freerouting.rules.DefaultItemClearanceClasses;
import app.freerouting.rules.Net;
import app.freerouting.rules.NetClass;
import app.freerouting.rules.ViaInfo;
import app.freerouting.rules.ViaRule;
import app.freerouting.settings.LayerSettings;
import app.freerouting.settings.sources.ApiSettings;
import app.freerouting.settings.sources.CliSettings;
import app.freerouting.settings.sources.JsonFileSettings;
import app.freerouting.settings.sources.RulesFileSettings;
import com.google.gson.JsonArray;
import com.google.gson.JsonNull;
import com.google.gson.JsonPrimitive;
import java.io.ByteArrayInputStream;
import java.io.ByteArrayOutputStream;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.Map;
import java.util.Set;
import java.util.TreeMap;
import java.util.TreeSet;

/**
 * Dumps reference outputs from the Java Freerouting engine for every DSN fixture, so the Rust
 * port can be compared against them.
 *
 * <pre>
 * ReferenceDump load  FIXTURES_DIR OUT_DIR
 * ReferenceDump route FIXTURES_DIR OUT_DIR [BOUNDS|-] [SAFETY_TIMEOUT hh:mm:ss|-] [NAME_FILTER]
 * </pre>
 *
 * <p>{@code load} parses each board with {@code DsnReader} and writes its {@code BoardStatistics}
 * (item counts, nets, incompletes, clearance violations). {@code route} runs the batch autorouter
 * the way the CLI does, with the optimizer off and bounded by work (BOUNDS: a route-bounds.tsv, a
 * pass limit for every board, or {@code -} for the file in {@code -Dreference.routeBounds}), and
 * writes the SES output plus statistics, score, per-pass scores and work-limit cut-offs. A board
 * whose safety timeout (default 30 min) fires gets no reference and makes the run exit with 1.
 * {@code REFERENCE_SHARD=k/n} routes every n-th board starting at the k-th. NAME_FILTER keeps the
 * boards whose path contains it, or any of its {@code |}-separated parts.
 */
public final class ReferenceDump {

  private static final Gson GSON = new GsonBuilder().setPrettyPrinting().serializeNulls().create();

  private ReferenceDump() {}

  public static void main(String[] args) throws Exception {
    if (args.length < 3) {
      System.err.println("usage: ReferenceDump load|route FIXTURES_DIR OUT_DIR [...]");
      System.exit(2);
    }
    String mode = args[0];
    Path fixtures = Path.of(args[1]).toAbsolutePath().normalize();
    Path out = Path.of(args[2]).toAbsolutePath().normalize();
    Freerouting.globalSettings = new GlobalSettings();
    if (ExtraModes.MODES.contains(mode)) {
      ExtraModes.run(mode, fixtures, out, args);
      System.exit(0);
    }

    List<Path> boards;
    try (Stream<Path> files = Files.walk(fixtures)) {
      boards =
          files
              .filter(p -> p.getFileName().toString().toLowerCase().endsWith(".dsn"))
              // fixtures/reference holds dumps (dsn/*.dsn), not input boards; fixtures/kicad
              // holds KiCad boards and their exports, which K07 compares itself
              .filter(p -> !fixtures.relativize(p).startsWith("reference"))
              .filter(p -> !fixtures.relativize(p).startsWith("kicad"))
              .sorted()
              .toList();
    }
    String filter = args.length > 5 ? args[5] : null;
    RouteBounds bounds = null;
    String safetyTimeout = null;
    int[] shard = parseShard(System.getenv("REFERENCE_SHARD"));
    if (mode.equals("route")) {
      bounds = RouteBounds.parse(args.length > 3 ? args[3] : "-");
      safetyTimeout = args.length > 4 && !args[4].equals("-") ? args[4] : DEFAULT_SAFETY_TIMEOUT;
    }

    int index = -1;
    List<String> failures = new ArrayList<>();
    for (Path board : boards) {
      String rel = fixtures.relativize(board).toString().replace('\\', '/');
      if (filter != null && Arrays.stream(filter.split("\\|")).noneMatch(rel::contains)) {
        continue;
      }
      index++;
      if (index % shard[1] != shard[0]) {
        continue;
      }
      Path target = out.resolve(mode).resolve(rel.replaceAll("\\.dsn$", ".json"));
      if (mode.equals("route") && Files.exists(target) && System.getenv("REFERENCE_REDO") == null) {
        continue;
      }
      System.out.println(mode + ": " + rel);
      JsonObject result;
      Path sesOut = out.resolve("route").resolve(rel.replaceAll("\\.dsn$", ".ses"));
      try {
        result =
            switch (mode) {
              case "load" -> load(board);
              case "route" -> route(board, rel, sesOut, bounds, safetyTimeout);
              default -> throw new IllegalArgumentException("unknown mode " + mode);
            };
      } catch (HarnessFailure f) {
        // No reference for this board: a stale one would pass for a current one.
        Files.deleteIfExists(target);
        Files.deleteIfExists(sesOut);
        System.err.println("route FAILED: " + rel + ": " + f.getMessage());
        failures.add(rel + ": " + f.getMessage());
        continue;
      } catch (Throwable t) {
        result = new JsonObject();
        result.addProperty("result", "exception");
        result.addProperty("error", t.toString());
      }
      result.addProperty("fixture", rel);
      Files.createDirectories(target.getParent());
      Files.writeString(target, GSON.toJson(result) + "\n", StandardCharsets.UTF_8);
    }
    if (!failures.isEmpty()) {
      System.err.println(failures.size() + " board(s) failed, no reference written:");
      failures.forEach(f -> System.err.println("  " + f));
      System.exit(1);
    }
    System.exit(0);
  }

  /** {@code REFERENCE_SHARD=k/n} routes only the boards whose index modulo n is k. */
  private static int[] parseShard(String value) {
    if (value == null || value.isEmpty()) {
      return new int[] {0, 1};
    }
    String[] parts = value.split("/");
    int k = Integer.parseInt(parts[0]);
    int n = Integer.parseInt(parts[1]);
    if (n < 1 || k < 0 || k >= n) {
      throw new IllegalArgumentException("REFERENCE_SHARD must be k/n with 0 <= k < n: " + value);
    }
    return new int[] {k, n};
  }

  private static JsonObject load(Path board) throws IOException {
    JsonObject result = new JsonObject();
    BoardReadResult read;
    try (InputStream in = new FileInputStream(board.toFile())) {
      read =
          DsnReader.readBoard(
              in, null, new ItemIdGenerator(), board.getFileName().toString());
    }
    BasicBoard basicBoard = null;
    switch (read) {
      case BoardReadResult.Success s -> {
        result.addProperty("result", "success");
        result.add("warnings", GSON.toJsonTree(s.warnings()));
        basicBoard = s.board();
      }
      case BoardReadResult.OutlineMissing s -> {
        result.addProperty("result", "outline_missing");
        result.add("warnings", GSON.toJsonTree(s.warnings()));
        basicBoard = s.board();
      }
      case BoardReadResult.ParseError e -> {
        result.addProperty("result", "parse_error");
        result.addProperty("location", e.location());
        result.addProperty("error", e.detail());
      }
      case BoardReadResult.IoError e -> {
        result.addProperty("result", "io_error");
        result.addProperty("error", String.valueOf(e.cause()));
      }
      default -> result.addProperty("result", "unknown");
    }
    if (basicBoard != null) {
      result.add("statistics", statistics(new BoardStatistics(basicBoard)));
    }
    return result;
  }

  /** Safety stop for one board's route run; a board that reaches it gets no reference. */
  private static final String DEFAULT_SAFETY_TIMEOUT = "00:30:00";

  /** At most this many WORK_LIMIT_CUTOFF lines are stored per board (the count is complete). */
  private static final int MAX_CUTOFF_LINES = 50;

  /** The machine-readable line a firing WorkLimit logs (WorkLimit.reportCutoff). */
  private static final Pattern CUTOFF =
      Pattern.compile("WORK_LIMIT_CUTOFF site=(\\S+) nominal_ms=(-?\\d+) units=(\\d+)(?: (.*))?$");

  /** One key=value pair of a cut-off line's context; values such as nets=[a, b] hold spaces. */
  private static final Pattern CONTEXT_PAIR = Pattern.compile("(\\w+)=(.*?)(?= \\w+=|$)");

  /** AutorouteBatchLoop's line at the end of each batch pass (the timing part is dropped). */
  private static final Pattern PASS_DONE =
      Pattern.compile(
          "Auto-routing pass #(\\d+) on board '([^']*)' was completed in \\S+ seconds with score"
              + " (.*?)(?:, using .*|\\.)$");

  /** A board run the harness could not turn into a reference (the safety timeout fired). */
  private static final class HarnessFailure extends Exception {
    HarnessFailure(String message) {
      super(message);
    }
  }

  /** One row of route-bounds.tsv: the work bounds a board routes with. */
  private record Bound(int maxPasses, Integer maxItems) {}

  /**
   * The per-board route bounds: either a route-bounds.tsv (tab separated, # comments, header
   * {@code board max_passes [max_items]}, board = file stem or path relative to the fixtures
   * directory) or one pass limit for every board.
   */
  private record RouteBounds(Map<String, Bound> rows, Bound uniform) {
    static RouteBounds parse(String arg) throws IOException {
      String spec = arg.equals("-") ? System.getProperty("reference.routeBounds") : arg;
      if (spec == null) {
        throw new IllegalArgumentException("route needs a bounds file or a pass limit");
      }
      if (spec.matches("\\d+")) {
        return new RouteBounds(Map.of(), new Bound(Integer.parseInt(spec), null));
      }
      Map<String, Bound> rows = new TreeMap<>();
      List<String> lines =
          Files.readAllLines(Path.of(spec)).stream()
              .filter(l -> !l.isBlank() && !l.startsWith("#"))
              .toList();
      List<String> header = Arrays.asList(lines.get(0).split("\t"));
      int board = header.indexOf("board");
      int passes = header.indexOf("max_passes");
      int items = header.indexOf("max_items");
      if (board < 0 || passes < 0) {
        throw new IllegalArgumentException(spec + " needs 'board' and 'max_passes' columns");
      }
      for (String line : lines.subList(1, lines.size())) {
        String[] cells = line.split("\t");
        String itemCell = items >= 0 && items < cells.length ? cells[items].trim() : "";
        Integer maxItems =
            itemCell.isEmpty() || itemCell.equals("-") ? null : Integer.valueOf(itemCell);
        rows.put(cells[board].trim(), new Bound(Integer.parseInt(cells[passes].trim()), maxItems));
      }
      return new RouteBounds(rows, null);
    }

    Bound forBoard(String rel) throws HarnessFailure {
      if (uniform != null) {
        return uniform;
      }
      String stem = Path.of(rel).getFileName().toString().replaceAll("\\.dsn$", "");
      for (String key : List.of(rel, rel.replaceAll("\\.dsn$", ""), stem)) {
        Bound b = rows.get(key);
        if (b != null) {
          return b;
        }
      }
      throw new HarnessFailure("no row in the route bounds");
    }
  }

  private static JsonObject route(
      Path board, String rel, Path sesOut, RouteBounds bounds, String safetyTimeout)
      throws Exception {
    Bound bound = bounds.forBoard(rel);
    JsonObject result = new JsonObject();
    Session session =
        SessionManager.getInstance()
            .createSession(UUID.randomUUID(), "Freerouting/" + Freerouting.VERSION_NUMBER_STRING);
    RoutingJob job = new RoutingJob(session.id);
    job.setInput(board.toFile());

    SettingsMerger merger =
        new SettingsMerger(
            new DefaultSettings(),
            new DsnFileSettings(job.input.getData(), job.input.getFilename()));
    merger.addOrReplaceSources(new Overrides(bound, safetyTimeout));
    job.routerSettings = merger.merge();

    FRLogger.getLogEntries().clear();
    RoutingJobScheduler scheduler = RoutingJobScheduler.getInstance();
    long start = System.nanoTime();
    scheduler.enqueueJob(job);
    job.state = RoutingJobState.READY_TO_START;
    while (job.state != RoutingJobState.COMPLETED
        && job.state != RoutingJobState.CANCELLED
        && job.state != RoutingJobState.TERMINATED
        && job.state != RoutingJobState.TIMED_OUT) {
      Thread.sleep(50);
    }
    long routeMillis = (System.nanoTime() - start) / 1_000_000;
    synchronized (scheduler.jobs) {
      scheduler.jobs.clear();
    }
    if (job.state == RoutingJobState.TIMED_OUT) {
      throw new HarnessFailure(
          "the safety timeout of " + safetyTimeout + " fired after " + routeMillis + " ms");
    }
    result.addProperty("result", job.state.toString().toLowerCase());
    result.addProperty("routeMillis", routeMillis);
    result.addProperty("freeroutingVersion", app.freerouting.constants.Constants.FREEROUTING_VERSION);
    result.addProperty(
        "javaRuntime",
        System.getProperty("java.vm.name") + " " + System.getProperty("java.runtime.version"));
    result.addProperty("maxPasses", bound.maxPasses());
    result.addProperty("maxItems", bound.maxItems());
    result.addProperty("safetyTimeout", safetyTimeout);
    result.add("settings", ExtraModes.settingsTree(job.routerSettings));
    if (job.board != null) {
      result.add("effective", ExtraModes.effective(job.routerSettings, job.board));
    }
    addLogFields(result);
    if (job.board != null) {
      BoardStatistics stats = job.board.getStatistics();
      result.add("statistics", statistics(stats));
      result.addProperty("routerScore", stats.getRouterScore(job.routerSettings));
    }
    if (job.output != null && job.output.getData() != null) {
      Files.createDirectories(sesOut.getParent());
      try (InputStream ses = job.output.getData()) {
        Files.write(sesOut, ses.readAllBytes());
      }
    }
    return result;
  }

  /**
   * Adds the per-pass score lines and the work-limit cut-offs from the job's log. A cut-off line
   * belongs to the pass whose "was completed" line follows it.
   */
  private static void addLogFields(JsonObject result) {
    JsonArray passes = new JsonArray();
    JsonArray lines = new JsonArray();
    int cutoffs = 0;
    int completedPasses = 0;
    for (LogEntry le : FRLogger.getLogEntries().getEntries(null, null)) {
      String message = le.getMessage();
      if (message == null) {
        continue;
      }
      Matcher pass = PASS_DONE.matcher(message);
      if (pass.find()) {
        completedPasses++;
        JsonObject p = new JsonObject();
        p.addProperty("pass", Integer.parseInt(pass.group(1)));
        p.addProperty("boardHash", pass.group(2));
        p.addProperty("score", pass.group(3));
        passes.add(p);
        continue;
      }
      Matcher cut = CUTOFF.matcher(message);
      if (!cut.find()) {
        continue;
      }
      cutoffs++;
      if (lines.size() >= MAX_CUTOFF_LINES) {
        continue;
      }
      JsonObject c = new JsonObject();
      c.addProperty("pass", completedPasses + 1);
      c.addProperty("site", cut.group(1));
      c.addProperty("nominalMs", Long.parseLong(cut.group(2)));
      c.addProperty("units", Long.parseLong(cut.group(3)));
      JsonObject context = new JsonObject();
      if (cut.group(4) != null) {
        Matcher kv = CONTEXT_PAIR.matcher(cut.group(4));
        while (kv.find()) {
          context.addProperty(kv.group(1), kv.group(2));
        }
      }
      c.add("context", context);
      c.addProperty("line", message.substring(cut.start()));
      lines.add(c);
    }
    result.add("passes", passes);
    JsonObject w = new JsonObject();
    w.addProperty("count", cutoffs);
    w.add("lines", lines);
    result.add("workLimitCutoffs", w);
  }

  private static JsonElement statistics(BoardStatistics stats) {
    return JsonParser.parseString(stats.toString());
  }

  /**
   * Highest-priority settings source carrying the harness's work bounds: the autorouter only (the
   * optimizer off), max_passes and max_items from the bounds, and the safety timeout.
   */
  private static final class Overrides implements SettingsSource {
    private final RouterSettings settings = new RouterSettings();

    Overrides(Bound bound, String safetyTimeout) {
      settings.optimizer.enabled = false;
      settings.scoring = null;
      settings.autorouter.maxPasses = bound.maxPasses();
      if (bound.maxItems() != null) {
        settings.autorouter.maxItems = bound.maxItems();
      }
      settings.jobTimeoutString = safetyTimeout;
    }

    @Override
    public RouterSettings getSettings() {
      return settings;
    }

    @Override
    public String getSourceName() {
      return "Reference harness";
    }

    @Override
    public int getPriority() {
      return 80;
    }
  }

  // ---------------------------------------------------------------------------------------------
  // C01 modes: settings, argv, ses, dsn, rules, clirun.
  //
  //   ReferenceDump settings FIXTURES_DIR OUT_DIR [NAME_FILTER]
  //   ReferenceDump ses      FIXTURES_DIR OUT_DIR [NAME_FILTER]
  //   ReferenceDump dsn      FIXTURES_DIR OUT_DIR [NAME_FILTER]
  //   ReferenceDump rules    FIXTURES_DIR OUT_DIR [NAME_FILTER]
  //   ReferenceDump argv     FIXTURES_DIR OUT_DIR ARGV_FILE
  //   ReferenceDump clirun   FIXTURES_DIR OUT_DIR CLIRUN_TSV [ID_FILTER]
  //
  // Every mode is deterministic: two runs give byte-identical output. See
  // tools/java-reference/README.md for the output layout, the scenarios and the subset rule.
  // ---------------------------------------------------------------------------------------------

  /** The C01 modes. Kept in one nested class so edits to the older modes never touch it. */
  static final class ExtraModes {

    static final Set<String> MODES = Set.of("settings", "argv", "ses", "dsn", "rules", "clirun");

    /** Plain Gson: full-precision doubles, nulls kept (GsonProvider.GSON rounds to 2 decimals). */
    static final Gson PLAIN = new GsonBuilder().serializeNulls().create();

    /** Plain Gson that drops nulls (used to list only the non-null fields). */
    static final Gson NO_NULLS = new GsonBuilder().create();

    static final int CORES = Runtime.getRuntime().availableProcessors();

    private ExtraModes() {}

    static void run(String mode, Path fixtures, Path out, String[] args) throws Exception {
      String extra = args.length > 3 ? args[3] : null;
      switch (mode) {
        case "argv" -> argvMode(Path.of(requireArg(extra, "ARGV_FILE")), out);
        case "clirun" ->
            clirunMode(
                Path.of(requireArg(extra, "CLIRUN_TSV")),
                fixtures,
                out,
                args.length > 4 ? args[4] : null);
        default -> boardModes(mode, fixtures, out, extra);
      }
    }

    private static String requireArg(String value, String name) {
      if (value == null) {
        throw new IllegalArgumentException("missing argument " + name);
      }
      return value;
    }

    // ----------------------------------------------------------------------------------------
    // JSON output: Gson-free pretty printer that keeps arrays of primitives on one line, so the
    // clearance matrices stay readable and small. Output is deterministic.
    // ----------------------------------------------------------------------------------------

    static String pretty(JsonElement e) {
      StringBuilder sb = new StringBuilder();
      writeJson(e, sb, 0);
      sb.append('\n');
      return sb.toString();
    }

    private static void indent(StringBuilder sb, int n) {
      for (int i = 0; i < n; i++) {
        sb.append("  ");
      }
    }

    private static boolean isScalar(JsonElement e) {
      return e == null || e.isJsonNull() || e.isJsonPrimitive();
    }

    private static void writeJson(JsonElement e, StringBuilder sb, int depth) {
      if (e == null || e.isJsonNull()) {
        sb.append("null");
      } else if (e.isJsonPrimitive()) {
        sb.append(e.getAsJsonPrimitive().toString());
      } else if (e.isJsonArray()) {
        JsonArray a = e.getAsJsonArray();
        if (a.isEmpty()) {
          sb.append("[]");
          return;
        }
        boolean flat = true;
        for (JsonElement x : a) {
          flat &= isScalar(x);
        }
        if (flat) {
          sb.append('[');
          for (int i = 0; i < a.size(); i++) {
            if (i > 0) {
              sb.append(", ");
            }
            writeJson(a.get(i), sb, depth + 1);
          }
          sb.append(']');
        } else {
          sb.append("[\n");
          for (int i = 0; i < a.size(); i++) {
            indent(sb, depth + 1);
            writeJson(a.get(i), sb, depth + 1);
            sb.append(i + 1 < a.size() ? ",\n" : "\n");
          }
          indent(sb, depth);
          sb.append(']');
        }
      } else {
        JsonObject o = e.getAsJsonObject();
        if (o.size() == 0) {
          sb.append("{}");
          return;
        }
        sb.append("{\n");
        int i = 0;
        for (Map.Entry<String, JsonElement> en : o.entrySet()) {
          indent(sb, depth + 1);
          sb.append(new JsonPrimitive(en.getKey()).toString()).append(": ");
          writeJson(en.getValue(), sb, depth + 1);
          sb.append(++i < o.size() ? ",\n" : "\n");
        }
        indent(sb, depth);
        sb.append('}');
      }
    }

    static void writeText(Path target, String text) throws IOException {
      Files.createDirectories(target.getParent());
      Files.writeString(target, text, StandardCharsets.UTF_8);
    }

    static void writeBytes(Path target, byte[] bytes) throws IOException {
      Files.createDirectories(target.getParent());
      Files.write(target, bytes);
    }

    static JsonArray strings(Iterable<String> values) {
      JsonArray a = new JsonArray();
      for (String s : values) {
        a.add(s);
      }
      return a;
    }

    // ----------------------------------------------------------------------------------------
    // Board modes: settings, ses, dsn, rules
    // ----------------------------------------------------------------------------------------

    /** One board to dump: the DSN file, the key used in output paths (no extension) and kind. */
    record BoardRef(Path dsn, String name, boolean isCase) {}

    static List<BoardRef> collectBoards(Path fixtures, String mode, String filter)
        throws IOException {
      List<BoardRef> result = new ArrayList<>();
      try (Stream<Path> files = Files.walk(fixtures)) {
        for (Path p : files.sorted().toList()) {
          String n = p.getFileName().toString();
          if (!n.toLowerCase().endsWith(".dsn") || fixtures.relativize(p).startsWith("reference")) {
            continue;
          }
          String rel = fixtures.relativize(p).toString().replace('\\', '/');
          result.add(new BoardRef(p, rel.replaceAll("\\.dsn$", ""), false));
        }
      }
      if (mode.equals("settings") || mode.equals("ses") || mode.equals("dsn")) {
        Path cases = casesDir(fixtures);
        if (Files.isDirectory(cases)) {
          try (Stream<Path> files = Files.list(cases)) {
            for (Path p : files.sorted().toList()) {
              String n = p.getFileName().toString();
              if (n.toLowerCase().endsWith(".dsn")) {
                String stem = n.replaceAll("\\.dsn$", "");
                result.add(new BoardRef(p, mode.equals("settings") ? stem : "cases/" + stem, true));
              }
            }
          }
        }
      }
      if (filter != null) {
        result.removeIf(b -> !b.name().contains(filter));
      }
      return result;
    }

    static Path casesDir(Path fixtures) {
      String prop = System.getProperty("reference.casesDir");
      if (prop != null && !prop.isEmpty()) {
        return Path.of(prop).toAbsolutePath().normalize();
      }
      return fixtures.resolve("../tools/java-reference/cases").normalize();
    }

    static Path subsetFile(Path fixtures) {
      return casesDir(fixtures).resolveSibling("dump-subset.txt");
    }

    static Set<String> readSubset(Path fixtures) throws IOException {
      Set<String> names = new TreeSet<>();
      for (String line : Files.readAllLines(subsetFile(fixtures))) {
        String t = line.strip();
        if (t.isEmpty() || t.startsWith("#")) {
          continue;
        }
        names.add(t.split("\t", 2)[0].strip());
      }
      return names;
    }

    static void boardModes(String mode, Path fixtures, Path out, String filter)
        throws Exception {
      boolean full = System.getenv("REFERENCE_FULL") != null;
      boolean subsetOnly = (mode.equals("ses") || mode.equals("dsn")) && !full;
      Set<String> subset = subsetOnly ? readSubset(fixtures) : null;
      List<BoardRef> boards = collectBoards(fixtures, mode, filter);
      long start = System.nanoTime();
      int done = 0;
      for (BoardRef b : boards) {
        if (subsetOnly && !subset.contains(b.name())) {
          continue;
        }
        System.out.println(mode + ": " + (b.isCase() ? "cases/" : "") + b.name());
        try {
          switch (mode) {
            case "settings" -> settingsBoard(b, fixtures, out);
            case "ses" -> sesBoard(b, fixtures, out);
            case "dsn" -> dsnBoard(b, out);
            case "rules" -> rulesBoard(b, out);
            default -> throw new IllegalArgumentException("unknown mode " + mode);
          }
        } catch (Throwable t) {
          JsonObject r = new JsonObject();
          r.addProperty("result", "exception");
          r.addProperty("error", t.toString());
          r.addProperty("fixture", b.name() + ".dsn");
          Path target =
              mode.equals("settings")
                  ? settingsTarget(b, out)
                  : out.resolve(mode).resolve(b.name() + ".json");
          writeText(target, pretty(r));
        }
        done++;
      }
      System.out.println(
          mode + ": " + done + " boards in " + (System.nanoTime() - start) / 1_000_000_000L + " s");
      if (subsetOnly) {
        for (String s : subset) {
          if (boards.stream().noneMatch(b -> b.name().equals(s)) && filter == null) {
            throw new IllegalStateException("dump-subset.txt names a missing board: " + s);
          }
        }
      }
    }

    static Path settingsTarget(BoardRef b, Path out) {
      return b.isCase()
          ? out.resolve("cases").resolve("settings").resolve(b.name() + ".json")
          : out.resolve("settings").resolve(b.name() + ".json");
    }

    // ----------------------------------------------------------------------------------------
    // prepareLikeScheduler: CliRunner.initializeCli's first merge, then the load sequence of
    // RoutingJobScheduler (java:92-191) call for call, without starting the router thread.
    // The one change: awaitPostLoad() right after the load, so the deferred post-load thread
    // cannot race the rules import.
    // ----------------------------------------------------------------------------------------

    static final class Prep {
      RoutingJob job;
      RoutingBoard board;
      JsonObject failure;
      String name;
      String adjacentRules;
      String rulesError;

      /** First net class other than "default", right after the DSN load (before any rules). */
      String firstClassAfterLoad;
    }

    static SettingsMerger protoDefault() {
      return new SettingsMerger(new DefaultSettings());
    }

    /** Java's TestingSettings (src/test): priority 80, only copperToEdgeClearanceUm = 0. */
    static final class FixtureOverride implements SettingsSource {
      private final RouterSettings settings = new RouterSettings();

      FixtureOverride() {
        settings.optimizer = null;
        settings.scoring = null;
        settings.copperToEdgeClearanceUm = 0.0;
      }

      @Override
      public RouterSettings getSettings() {
        return settings;
      }

      @Override
      public String getSourceName() {
        return "Reference fixture override";
      }

      @Override
      public int getPriority() {
        return 80;
      }
    }

    static SettingsMerger protoFixture() {
      return new SettingsMerger(new DefaultSettings(), new FixtureOverride());
    }

    static String[] cliArgs(String base, String incClass) {
      return new String[] {
        "-de", base + ".dsn", "-do", base + ".ses", "-mp", "7", "-mt", "3", "-oit", "0.035",
        "-inc", incClass, "--router.via_costs=77", "--router.scoring.plane_via_costs=7",
        "--router.optimizer.enabled=false",
        "--router.default_undesired_direction_trace_cost=1.5"
      };
    }

    static SettingsMerger protoCli(String base, String incClass) {
      return new SettingsMerger(new DefaultSettings(), new CliSettings(cliArgs(base, incClass)));
    }

    static SettingsMerger protoOwnerConfig(Path ownerJson) {
      return new SettingsMerger(new DefaultSettings(), new JsonFileSettings(ownerJson));
    }

    static Prep prepare(Path dsn, SettingsMerger prototype) throws Exception {
      Prep p = new Prep();
      GlobalSettings gs = Freerouting.globalSettings;
      gs.settingsMergerProtype = prototype;
      gs.initialRulesFile = null;
      gs.designSessionFilename = null;
      Session session =
          SessionManager.getInstance()
              .createSession(UUID.randomUUID(), "Freerouting/" + Freerouting.VERSION_NUMBER_STRING);
      RoutingJob job = new RoutingJob(session.id);
      job.setInput(dsn.toFile());
      p.job = job;
      if (job.input == null || job.input.format != FileFormat.DSN) {
        p.failure = new JsonObject();
        p.failure.addProperty("result", "not_dsn");
        return p;
      }
      p.name = job.input.getFilenameWithoutExtension();

      // CliRunner.initializeCli: first merge (prototype + DsnFileSettings).
      SettingsMerger first = gs.settingsMergerProtype.clone();
      first.addOrReplaceSources(new DsnFileSettings(job.input.getData(), job.input.getFilename()));
      job.routerSettings = first.merge();
      job.drcSettings = gs.drcSettings.clone();

      // RoutingJobScheduler: load the board.
      HeadlessBoardManager boardManager = new HeadlessBoardManager(job);
      BoardReadResult read =
          boardManager.loadFromSpecctraDsn(job.input.getData(), null, new ItemIdGenerator());
      job.board = boardManager.getRoutingBoard();
      if (job.board == null) {
        p.failure = new JsonObject();
        switch (read) {
          case BoardReadResult.ParseError e -> {
            p.failure.addProperty("result", "parse_error");
            p.failure.addProperty("location", e.location());
            p.failure.addProperty("error", e.detail());
          }
          case BoardReadResult.IoError e -> {
            p.failure.addProperty("result", "io_error");
            p.failure.addProperty("error", String.valueOf(e.cause()));
          }
          default -> p.failure.addProperty("result", "no_board");
        }
        return p;
      }
      job.board.awaitPostLoad();
      p.board = job.board;
      p.firstClassAfterLoad = "POWER";
      for (int i = 0; i < job.board.rules.netClasses.count(); i++) {
        String n = job.board.rules.netClasses.get(i).getName();
        if (!n.equalsIgnoreCase("default")) {
          p.firstClassAfterLoad = n;
          break;
        }
      }

      // RoutingJobScheduler: second merge (prototype + DSN + adjacent rules + the job's settings).
      SettingsMerger second = gs.settingsMergerProtype.clone();
      second.addOrReplaceSources(new DsnFileSettings(job.input.getData(), job.input.getFilename()));
      byte[] rulesData = null;
      String rulesFilename = null;
      if (job.input.getDirectoryPath() != null) {
        String baseName = job.input.getFilename();
        if (baseName.lastIndexOf('.') > 0) {
          baseName = baseName.substring(0, baseName.lastIndexOf('.'));
        }
        java.io.File autoRules = new java.io.File(job.input.getDirectoryPath(), baseName + ".rules");
        if (autoRules.exists()) {
          rulesData = Files.readAllBytes(autoRules.toPath());
          rulesFilename = autoRules.getName();
        }
      }
      if (rulesData != null) {
        second.addOrReplaceSources(
            new RulesFileSettings(new ByteArrayInputStream(rulesData), rulesFilename));
      }
      second.addOrReplaceSources(new ApiSettings(job.routerSettings));
      job.routerSettings = second.merge();
      p.adjacentRules = rulesFilename;

      if (rulesData != null) {
        try {
          RulesReader.read(
              new ByteArrayInputStream(rulesData),
              job.name != null ? job.name : "board",
              job.board,
              job.routerSettings);
        } catch (Exception e) {
          p.rulesError = e.toString();
        }
      }
      job.routerSettings.applyBoardSpecificOptimizations(job.board);
      job.routerSettings.applyNetClassExclusions(job.board);
      // (The scheduler then imports a session file when one is configured; none is here.)
      return p;
    }

    // ----------------------------------------------------------------------------------------
    // settings
    // ----------------------------------------------------------------------------------------

    static void settingsBoard(BoardRef b, Path fixtures, Path out) throws Exception {
      JsonObject result = new JsonObject();
      result.addProperty("fixture", b.name() + ".dsn");
      JsonObject scenarios = new JsonObject();
      result.add("scenarios", scenarios);

      Prep dflt = prepare(b.dsn(), protoDefault());
      if (dflt.failure != null) {
        for (String s : List.of("default", "fixture", "cli", "owner-config")) {
          scenarios.add(s, dflt.failure.deepCopy());
        }
        writeText(settingsTarget(b, out), pretty(result));
        return;
      }
      String base = dflt.name;
      String incClass = dflt.firstClassAfterLoad;
      scenarios.add("default", scenarioJson(dflt));
      dflt = null;
      scenarios.add("fixture", scenarioJson(prepare(b.dsn(), protoFixture())));
      JsonObject cli = scenarioJson(prepare(b.dsn(), protoCli(base, incClass)));
      JsonObject cliWithClass = new JsonObject();
      cliWithClass.addProperty("incClass", incClass);
      for (Map.Entry<String, JsonElement> e : cli.entrySet()) {
        cliWithClass.add(e.getKey(), e.getValue());
      }
      scenarios.add("cli", cliWithClass);
      Path owner = casesDir(fixtures).resolve("owner-freerouting.json");
      scenarios.add("owner-config", scenarioJson(prepare(b.dsn(), protoOwnerConfig(owner))));
      writeText(settingsTarget(b, out), pretty(result));
    }

    static JsonObject scenarioJson(Prep p) throws Exception {
      JsonObject o = new JsonObject();
      if (p.failure != null) {
        return p.failure;
      }
      RouterSettings rs = p.job.routerSettings;
      RoutingBoard board = p.board;
      o.add("adjacentRules", p.adjacentRules == null ? JsonNull.INSTANCE : new JsonPrimitive(p.adjacentRules));
      if (p.rulesError != null) {
        o.addProperty("rulesError", p.rulesError);
      }
      o.add("settings", settingsTree(rs));
      o.add("effective", effective(rs, board));
      o.add("board", boardJson(board));
      o.addProperty(
          "postLoadViolations",
          new DesignRulesChecker(board, null).getAllClearanceViolations().size());
      return o;
    }

    static JsonObject settingsTree(RouterSettings rs) {
      JsonObject t = PLAIN.toJsonTree(rs).getAsJsonObject();
      JsonArray layers = new JsonArray();
      if (rs.layers != null) {
        for (LayerSettings l : rs.layers) {
          layers.add(l == null ? JsonNull.INSTANCE : PLAIN.toJsonTree(l));
        }
      }
      t.add("layers", layers);
      if (rs.scoring != null && t.get("scoring") != null && t.get("scoring").isJsonObject()) {
        JsonObject s = t.getAsJsonObject("scoring");
        s.add("preferred_direction_trace_cost", doubles(rs.scoring.preferredDirectionTraceCost));
        s.add("undesired_direction_trace_cost", doubles(rs.scoring.undesiredDirectionTraceCost));
      }
      if (rs.optimizer != null
          && t.get("optimizer") != null
          && t.get("optimizer").isJsonObject()) {
        JsonObject s = t.getAsJsonObject("optimizer");
        s.addProperty(
            "board_update_strategy",
            rs.optimizer.boardUpdateStrategy == null ? null : rs.optimizer.boardUpdateStrategy.name());
        s.addProperty(
            "item_selection_strategy",
            rs.optimizer.itemSelectionStrategy == null
                ? null
                : rs.optimizer.itemSelectionStrategy.name());
      }
      normalizeThreads(t);
      return t;
    }

    static JsonElement doubles(double[] values) {
      if (values == null) {
        return JsonNull.INSTANCE;
      }
      JsonArray a = new JsonArray();
      for (double v : values) {
        a.add(v);
      }
      return a;
    }

    /** Machine-dependent thread counts become "<cores-1>" / "<cores>". */
    static void normalizeThreads(JsonElement e) {
      if (e.isJsonObject()) {
        JsonObject o = e.getAsJsonObject();
        for (Map.Entry<String, JsonElement> en : new ArrayList<>(o.entrySet())) {
          JsonElement v = en.getValue();
          if (en.getKey().equals("max_threads")
              && v.isJsonPrimitive()
              && v.getAsJsonPrimitive().isNumber()) {
            int n = v.getAsInt();
            if (n == Math.max(1, CORES - 1)) {
              o.addProperty("max_threads", "<cores-1>");
            } else if (n == CORES) {
              o.addProperty("max_threads", "<cores>");
            }
          } else {
            normalizeThreads(v);
          }
        }
      } else if (e.isJsonArray()) {
        for (JsonElement x : e.getAsJsonArray()) {
          normalizeThreads(x);
        }
      }
    }

    static JsonObject effective(RouterSettings rs, RoutingBoard board) {
      JsonObject e = new JsonObject();
      JsonArray layers = new JsonArray();
      for (int i = 0; i < board.getLayerCount(); i++) {
        JsonObject l = new JsonObject();
        l.addProperty("name", board.layerStructure.layers[i].name);
        l.addProperty("isSignal", board.layerStructure.layers[i].isSignal);
        l.addProperty("active", rs.getLayerActive(i));
        l.addProperty("preferredHorizontal", rs.getPreferredDirectionIsHorizontal(i));
        l.addProperty("bendCost", rs.getBendCost(i));
        l.addProperty("preferredCost", rs.getPreferredDirectionTraceCosts(i));
        l.addProperty("againstCost", rs.getAgainstPreferredDirectionTraceCosts(i));
        l.addProperty("horizontalCost", rs.getHorizontalTraceCosts(i));
        l.addProperty("verticalCost", rs.getVerticalTraceCosts(i));
        layers.add(l);
      }
      e.add("layers", layers);
      e.addProperty("boardSpecificTraceCostsApplied", rs.boardSpecificTraceCostsApplied);
      e.addProperty("viaCosts", rs.getViaCosts());
      e.addProperty("planeViaCosts", rs.getPlaneViaCosts());
      e.addProperty("startRipupCosts", rs.getStartRipupCosts());
      e.addProperty("runRouter", rs.getRunRouter());
      e.addProperty("runOptimizer", rs.getRunOptimizer());
      e.addProperty("runFanout", rs.getRunFanout());
      e.add(
          "validationWarnings",
          rs.validationWarnings == null ? JsonNull.INSTANCE : strings(rs.validationWarnings));
      return e;
    }

    static String className(ClearanceMatrix m, int index) {
      if (index < 0 || index >= m.getClassCount()) {
        return "#" + index;
      }
      return m.getName(index);
    }

    static JsonObject boardJson(RoutingBoard board) {
      JsonObject o = new JsonObject();
      BoardRules rules = board.rules;
      ClearanceMatrix m = rules.clearanceMatrix;

      JsonObject cm = new JsonObject();
      JsonArray names = new JsonArray();
      for (int i = 0; i < m.getClassCount(); i++) {
        names.add(m.getName(i));
      }
      cm.add("classNames", names);
      JsonArray perLayer = new JsonArray();
      for (int layer = 0; layer < m.getLayerCount(); layer++) {
        JsonArray rows = new JsonArray();
        for (int i = 0; i < m.getClassCount(); i++) {
          JsonArray row = new JsonArray();
          for (int j = 0; j < m.getClassCount(); j++) {
            row.add(m.getValue(i, j, layer, false));
          }
          rows.add(row);
        }
        perLayer.add(rows);
      }
      cm.add("valuesByLayerRowCol", perLayer);
      o.add("clearanceMatrix", cm);

      BoardOutline outline = board.getOutline();
      o.add(
          "outlineClearanceClass",
          outline == null
              ? JsonNull.INSTANCE
              : new JsonPrimitive(className(m, outline.clearanceClassIndex())));
      o.addProperty(
          "minimumPinGap", outline == null ? null : Double.toString(outline.minimumPinGap()));
      o.addProperty("holeClearance", rules.getHoleClearance());
      o.addProperty("clearanceToleranceUm", rules.clearanceToleranceUm);
      o.addProperty("ignoreConduction", rules.getIgnoreConduction());

      JsonArray planes = new JsonArray();
      for (int n = 1; n <= rules.nets.maxNetNumber(); n++) {
        Net net = rules.nets.get(n);
        if (net != null && net.containsPlane()) {
          planes.add(net.name);
        }
      }
      o.add("planeNets", planes);

      JsonArray classes = new JsonArray();
      for (int i = 0; i < rules.netClasses.count(); i++) {
        NetClass nc = rules.netClasses.get(i);
        JsonObject c = new JsonObject();
        c.addProperty("name", nc.getName());
        c.add(
            "viaRule",
            nc.getViaRule() == null ? JsonNull.INSTANCE : new JsonPrimitive(nc.getViaRule().name));
        c.addProperty("traceClearanceClass", className(m, nc.getTraceClearanceClass()));
        JsonArray hw = new JsonArray();
        JsonArray active = new JsonArray();
        for (int layer = 0; layer < board.getLayerCount(); layer++) {
          hw.add(nc.getTraceHalfWidth(layer));
          active.add(nc.isActiveRoutingLayer(layer));
        }
        c.add("traceHalfWidth", hw);
        c.add("activeRoutingLayer", active);
        c.addProperty("isIgnoredByAutorouter", nc.isIgnoredByAutorouter);
        c.addProperty("shoveFixed", nc.isShoveFixed());
        c.addProperty("pullTight", nc.getPullTight());
        c.addProperty("ignoreCyclesWithAreas", nc.getIgnoreCyclesWithAreas());
        c.addProperty("minimumTraceLength", nc.getMinimumTraceLength());
        c.addProperty("maximumTraceLength", nc.getMaximumTraceLength());
        JsonObject dic = new JsonObject();
        for (DefaultItemClearanceClasses.ItemClass ic :
            DefaultItemClearanceClasses.ItemClass.values()) {
          dic.addProperty(ic.name(), className(m, nc.defaultItemClearanceClasses.get(ic)));
        }
        c.add("defaultItemClearanceClasses", dic);
        classes.add(c);
      }
      o.add("netClasses", classes);

      JsonArray viaInfos = new JsonArray();
      for (int i = 0; i < rules.viaInfos.count(); i++) {
        ViaInfo vi = rules.viaInfos.get(i);
        JsonObject v = new JsonObject();
        v.addProperty("name", vi.getName());
        v.addProperty("padstack", vi.getPadstack() == null ? null : vi.getPadstack().name);
        v.addProperty("clearanceClass", className(m, vi.getClearanceClassIndex()));
        v.addProperty("attachSmd", vi.attachSmdAllowed());
        viaInfos.add(v);
      }
      o.add("viaInfos", viaInfos);

      JsonArray viaRules = new JsonArray();
      for (ViaRule vr : rules.viaRules) {
        JsonObject v = new JsonObject();
        v.addProperty("name", vr.name);
        JsonArray vias = new JsonArray();
        for (int i = 0; i < vr.viaCount(); i++) {
          vias.add(vr.getVia(i).getName());
        }
        v.add("vias", vias);
        viaRules.add(v);
      }
      o.add("viaRules", viaRules);

      o.addProperty("snapAngle", String.valueOf(rules.getTraceAngleRestriction()));
      if (board.library.padstacks == null) {
        o.add("padstacks", JsonNull.INSTANCE);
      } else {
        JsonArray pads = new JsonArray();
        for (int i = 1; i <= board.library.padstacks.count(); i++) {
          Padstack ps = board.library.padstacks.get(i);
          pads.add(ps == null ? JsonNull.INSTANCE : new JsonPrimitive(ps.name));
        }
        o.add("padstacks", pads);
      }
      JsonArray viaPads = new JsonArray();
      Padstack[] viaPadstacks =
          board.library.padstacks == null ? null : board.library.getViaPadstacks();
      if (viaPadstacks != null) {
        for (Padstack ps : viaPadstacks) {
          viaPads.add(ps == null ? JsonNull.INSTANCE : new JsonPrimitive(ps.name));
        }
      }
      o.add("viaPadstacks", viaPads);
      return o;
    }

    // ----------------------------------------------------------------------------------------
    // ses
    // ----------------------------------------------------------------------------------------

    static void sesBoard(BoardRef b, Path fixtures, Path out) throws Exception {
      JsonObject result = new JsonObject();
      result.addProperty("fixture", b.name() + ".dsn");
      Prep p = prepare(b.dsn(), protoDefault());
      Path jsonTarget = out.resolve("ses").resolve(b.name() + ".json");
      if (p.failure != null) {
        result.add("load", p.failure);
        writeText(jsonTarget, pretty(result));
        return;
      }
      ByteArrayOutputStream unrouted = new ByteArrayOutputStream();
      SesWriter.write(p.board, unrouted, p.name);
      writeBytes(out.resolve("ses").resolve("unrouted").resolve(b.name() + ".ses"), unrouted.toByteArray());
      result.addProperty("designName", p.name);
      result.addProperty("unroutedBytes", unrouted.size());
      Path routeSes = fixtures.resolve("reference").resolve("route").resolve(b.name() + ".ses");
      if (Files.exists(routeSes)) {
        Prep fresh = prepare(b.dsn(), protoDefault());
        JsonObject imp = new JsonObject();
        try (InputStream in = new FileInputStream(routeSes.toFile())) {
          SesImportSummary s = SesReader.read(in, fresh.board);
          imp.addProperty("wiresImported", s.wiresImported());
          imp.addProperty("viasImported", s.viasImported());
          imp.addProperty("errorsEncountered", s.errorsEncountered());
        } catch (Exception e) {
          imp.addProperty("exception", e.toString());
        }
        ByteArrayOutputStream routed = new ByteArrayOutputStream();
        SesWriter.write(fresh.board, routed, fresh.name);
        writeBytes(out.resolve("ses").resolve("routed").resolve(b.name() + ".ses"), routed.toByteArray());
        imp.addProperty("routedBytes", routed.size());
        result.add("import", imp);
      } else {
        result.add("import", JsonNull.INSTANCE);
      }
      writeText(jsonTarget, pretty(result));
    }

    // ----------------------------------------------------------------------------------------
    // dsn
    // ----------------------------------------------------------------------------------------

    static void dsnBoard(BoardRef b, Path out) throws Exception {
      JsonObject result = new JsonObject();
      result.addProperty("fixture", b.name() + ".dsn");
      Path jsonTarget = out.resolve("dsn").resolve(b.name() + ".json");
      BoardReadResult read;
      String fileName = b.dsn().getFileName().toString();
      try (InputStream in = new FileInputStream(b.dsn().toFile())) {
        read = DsnReader.readBoard(in, null, new ItemIdGenerator(), fileName);
      }
      BasicBoard board =
          switch (read) {
            case BoardReadResult.Success s -> s.board();
            case BoardReadResult.OutlineMissing s -> s.board();
            default -> null;
          };
      if (board == null) {
        result.addProperty("result", "no_board");
        writeText(jsonTarget, pretty(result));
        return;
      }
      String designName = fileName.replaceAll("\\.[^.]*$", "");
      ByteArrayOutputStream baos = new ByteArrayOutputStream();
      DsnWriter.write(board, baos, designName, false);
      writeBytes(out.resolve("dsn").resolve(b.name() + ".dsn"), baos.toByteArray());
      result.addProperty("result", "written");
      result.addProperty("designName", designName);
      result.addProperty("bytes", baos.size());
      writeText(jsonTarget, pretty(result));
    }

    // ----------------------------------------------------------------------------------------
    // rules
    // ----------------------------------------------------------------------------------------

    static void rulesBoard(BoardRef b, Path out) throws Exception {
      Prep p = prepare(b.dsn(), protoDefault());
      if (p.failure != null) {
        JsonObject result = new JsonObject();
        result.addProperty("fixture", b.name() + ".dsn");
        result.add("load", p.failure);
        writeText(out.resolve("rules").resolve(b.name() + ".json"), pretty(result));
        return;
      }
      ByteArrayOutputStream baos = new ByteArrayOutputStream();
      RulesWriter.write(p.board, p.job.routerSettings, baos, p.name);
      writeBytes(out.resolve("rules").resolve(b.name() + ".rules"), baos.toByteArray());
    }

    // ----------------------------------------------------------------------------------------
    // argv
    // ----------------------------------------------------------------------------------------

    static void argvMode(Path file, Path out) throws Exception {
      JsonArray entries = new JsonArray();
      int lineNo = 0;
      for (String raw : Files.readAllLines(file, StandardCharsets.UTF_8)) {
        lineNo++;
        if (raw.isBlank() || raw.startsWith("#")) {
          continue;
        }
        String[] args = raw.split("\t", -1);
        entries.add(argvEntry(lineNo, args));
      }
      JsonObject root = new JsonObject();
      root.addProperty("source", file.getFileName().toString());
      root.add("entries", entries);
      writeText(out.resolve("argv.json"), pretty(root));
      System.out.println("argv: " + entries.size() + " command lines");
    }

    static JsonObject argvEntry(int lineNo, String[] args) {
      JsonObject e = new JsonObject();
      e.addProperty("line", lineNo);
      JsonArray a = new JsonArray();
      for (String s : args) {
        a.add(s);
      }
      e.add("args", a);

      GlobalSettings fresh = new GlobalSettings();
      GlobalSettings gs = new GlobalSettings();
      FRLogger.getLogEntries().clear();
      gs.applyCommandLineArguments(args);
      JsonObject g = new JsonObject();
      g.addProperty("initialInputFile", gs.initialInputFile);
      g.addProperty("designSessionFilename", gs.designSessionFilename);
      g.addProperty("initialRulesFile", gs.initialRulesFile);
      g.addProperty("initialOutputFile", gs.initialOutputFile);
      g.add("additionalOutputFiles", strings(gs.additionalOutputFiles));
      if (gs.drcReportFile == null) {
        g.add("drcReportFile", JsonNull.INSTANCE);
      } else {
        JsonObject d = new JsonObject();
        d.addProperty("filename", gs.drcReportFile.getFilename());
        d.addProperty("format", String.valueOf(gs.drcReportFile.format));
        g.add("drcReportFile", d);
      }
      g.addProperty("showHelpOption", gs.showHelpOption);
      g.addProperty("locale", gs.currentLocale == null ? null : gs.currentLocale.toLanguageTag());
      g.addProperty("maxPasses", gs.getMaxPasses());
      // The getters NPE on null fields, so read the optimizer fields directly.
      g.addProperty("optimizerMaxThreads", gs.routerSettings.optimizer.maxThreads);
      g.addProperty(
          "boardUpdateStrategy", String.valueOf(gs.routerSettings.optimizer.boardUpdateStrategy));
      g.addProperty(
          "itemSelectionStrategy",
          String.valueOf(gs.routerSettings.optimizer.itemSelectionStrategy));
      JsonObject dbg = new JsonObject();
      dbg.addProperty("enableDetailedLogging", gs.debugSettings.enableDetailedLogging);
      dbg.addProperty("singleStepExecution", gs.debugSettings.singleStepExecution);
      dbg.addProperty("traceInsertionDelay", gs.debugSettings.traceInsertionDelay);
      dbg.add("filterByNet", strings(new TreeSet<>(gs.debugSettings.filterByNet)));
      g.add("debug", dbg);
      g.add("log", logJson());
      JsonObject changed = new JsonObject();
      diff("", PLAIN.toJsonTree(fresh), PLAIN.toJsonTree(gs), changed);
      g.add("changedFields", changed);
      e.add("globalSettings", g);

      FRLogger.getLogEntries().clear();
      CliSettings cs = new CliSettings(args);
      JsonObject c = new JsonObject();
      c.add("settings", prune(NO_NULLS.toJsonTree(cs.getSettings())));
      JsonObject parsed = new JsonObject();
      for (Map.Entry<String, String> en : new TreeMap<>(cs.getParsedArguments()).entrySet()) {
        parsed.addProperty(en.getKey(), en.getValue());
      }
      c.add("parsedArguments", parsed);
      c.add("log", logJson());
      e.add("cliSettings", c);
      return e;
    }

    static JsonObject logJson() {
      JsonObject o = new JsonObject();
      int warnings = 0;
      int errors = 0;
      JsonArray texts = new JsonArray();
      for (LogEntry le : FRLogger.getLogEntries().getEntries(null, null)) {
        String type = le.getType().name();
        if (type.equals("Warning")) {
          warnings++;
          texts.add("WARNING " + le.getMessage());
        } else if (type.equals("Error")) {
          errors++;
          texts.add("ERROR " + le.getMessage());
        }
      }
      o.addProperty("warnings", warnings);
      o.addProperty("errors", errors);
      o.add("texts", texts);
      return o;
    }

    /** Leaf paths whose value differs between two trees (arrays count as leaves). */
    static void diff(String path, JsonElement a, JsonElement b, JsonObject out) {
      if (a != null && b != null && a.isJsonObject() && b.isJsonObject()) {
        for (Map.Entry<String, JsonElement> en : b.getAsJsonObject().entrySet()) {
          diff(
              path.isEmpty() ? en.getKey() : path + "." + en.getKey(),
              a.getAsJsonObject().get(en.getKey()),
              en.getValue(),
              out);
        }
      } else if (a == null || !a.equals(b)) {
        out.add(path, b);
      }
    }

    /** Removes nulls and empty objects. */
    static JsonElement prune(JsonElement e) {
      if (e.isJsonObject()) {
        JsonObject o = new JsonObject();
        for (Map.Entry<String, JsonElement> en : e.getAsJsonObject().entrySet()) {
          if (en.getValue().isJsonNull()) {
            continue;
          }
          JsonElement v = prune(en.getValue());
          if (v.isJsonObject() && v.getAsJsonObject().size() == 0) {
            continue;
          }
          o.add(en.getKey(), v);
        }
        return o;
      }
      return e;
    }

    // ----------------------------------------------------------------------------------------
    // clirun
    // ----------------------------------------------------------------------------------------

    static final long CLI_LIMIT_SECONDS = 120;
    static final int TEXT_LIMIT_BYTES = 64 * 1024;

    static void clirunMode(Path tsv, Path fixtures, Path out, String idFilter) throws Exception {
      // The jdk4py wheel is unzipped without execute bits, so lib/jspawnhelper cannot start.
      // Fix the permission of the runtime under target/ (or fall back to the fork launcher).
      Path spawnHelper = Path.of(System.getProperty("java.home"), "lib", "jspawnhelper");
      if (Files.exists(spawnHelper) && !Files.isExecutable(spawnHelper)) {
        spawnHelper.toFile().setExecutable(true);
      }
      String javaBin = ProcessHandle.current().info().command().orElseThrow();
      String classpath = System.getProperty("java.class.path");
      String tmpBaseEnv = System.getenv("REFERENCE_TMP_DIR");
      Path tmpBase = tmpBaseEnv == null ? null : Path.of(tmpBaseEnv);
      for (String raw : Files.readAllLines(tsv, StandardCharsets.UTF_8)) {
        if (raw.isBlank() || raw.startsWith("#")) {
          continue;
        }
        String[] cols = raw.split("\t", -1);
        String id = cols[0];
        if (idFilter != null && !id.contains(idFilter)) {
          continue;
        }
        System.out.println("clirun: " + id);
        List<String> copy = new ArrayList<>();
        if (!cols[1].equals("-")) {
          for (String c : cols[1].split(",")) {
            copy.add(c.strip());
          }
        }
        List<String> rowArgs = new ArrayList<>(Arrays.asList(cols).subList(2, cols.length));
        JsonObject result = cliRow(javaBin, classpath, tmpBase, fixtures, id, copy, rowArgs);
        writeText(out.resolve("clirun").resolve(id + ".json"), pretty(result));
      }
    }

    static JsonObject cliRow(
        String javaBin,
        String classpath,
        Path tmpBase,
        Path fixtures,
        String id,
        List<String> copy,
        List<String> rowArgs)
        throws Exception {
      Path tmp =
          tmpBase == null
              ? Files.createTempDirectory("fr-clirun-")
              : Files.createTempDirectory(tmpBase, "fr-clirun-");
      try {
        for (String c : copy) {
          Files.copy(fixtures.resolve(c), tmp.resolve(Path.of(c).getFileName().toString()));
        }
        List<String> cmd = new ArrayList<>();
        cmd.add(javaBin);
        cmd.addAll(
            List.of(
                "-Xmx2g",
                "-Djava.awt.headless=true",
                "-Dfile.encoding=UTF-8",
                "-Dsun.jnu.encoding=UTF-8",
                "-cp",
                classpath,
                "app.freerouting.Freerouting",
                "--user_data_path=" + tmp.resolve("userdata"),
                "-da",
                "--gui.enabled=false",
                "--api_server.enabled=false",
                "--mcp_server.enabled=false"));
        cmd.addAll(rowArgs);
        ProcessBuilder pb = new ProcessBuilder(cmd).directory(tmp.toFile()).redirectErrorStream(true);
        pb.environment().remove("JAVA_TOOL_OPTIONS");
        Process proc = pb.start();
        ByteArrayOutputStream captured = new ByteArrayOutputStream();
        Thread pump =
            new Thread(
                () -> {
                  try (InputStream in = proc.getInputStream()) {
                    in.transferTo(captured);
                  } catch (IOException ignored) {
                    // the process was killed
                  }
                });
        pump.start();
        if (!proc.waitFor(CLI_LIMIT_SECONDS, java.util.concurrent.TimeUnit.SECONDS)) {
          proc.destroyForcibly();
          throw new IllegalStateException("clirun " + id + " exceeded " + CLI_LIMIT_SECONDS + " s");
        }
        pump.join();

        JsonObject r = new JsonObject();
        r.addProperty("id", id);
        r.add("args", strings(rowArgs));
        r.addProperty("exitCode", proc.exitValue());
        JsonArray files = new JsonArray();
        JsonObject texts = new JsonObject();
        try (Stream<Path> walk = Files.walk(tmp)) {
          for (Path f : walk.filter(Files::isRegularFile).sorted().toList()) {
            String rel = tmp.relativize(f).toString().replace('\\', '/');
            if (rel.equals("userdata") || rel.startsWith("userdata/")) {
              continue;
            }
            JsonObject fo = new JsonObject();
            fo.addProperty("path", rel);
            byte[] bytes = Files.readAllBytes(f);
            long size = bytes.length;
            String text = null;
            if (size < TEXT_LIMIT_BYTES * 100L) {
              text = new String(bytes, StandardCharsets.UTF_8);
              String normalized = TIMESTAMP.matcher(text).replaceAll("\"date\": \"<t>\"");
              if (!normalized.equals(text)) {
                // A DRC report carries a timestamp whose length varies; report the size with it
                // replaced by a fixed placeholder.
                text = normalized;
                size = text.getBytes(StandardCharsets.UTF_8).length;
                fo.addProperty("timestampNormalized", true);
              }
            }
            fo.addProperty("size", size);
            files.add(fo);
            if (text != null && size < TEXT_LIMIT_BYTES && !copy.contains(rel)) {
              texts.addProperty(rel, text);
            }
          }
        }
        r.add("files", files);
        r.add("texts", texts);
        JsonArray stages = new JsonArray();
        JsonArray logLines = new JsonArray();
        JsonArray plain = new JsonArray();
        String tmpPath = tmp.toString();
        for (String line : captured.toString(StandardCharsets.UTF_8).split("\\R")) {
          String l = line.replace(tmpPath, "<tmp>");
          if (l.contains("Auto-routing stage")) {
            stages.add(normalizeStageLine(l));
          }
          if (LOG_PREFIX.matcher(l).find()) {
            String body = LOG_PREFIX.matcher(l).replaceFirst("");
            if (body.startsWith("WARN") || body.startsWith("ERROR") || body.contains("output file")) {
              logLines.add(JOB_ID.matcher(body).replaceAll("[<job>]"));
            }
          } else if (!l.isBlank() && !l.startsWith("\tat ") && !l.startsWith("Caused by")) {
            plain.add(l);
          }
        }
        r.add("stageLines", stages);
        r.add("logLines", logLines);
        r.add("plainOutput", plain);
        return r;
      } finally {
        try (Stream<Path> walk = Files.walk(tmp)) {
          for (Path f : walk.sorted(java.util.Comparator.reverseOrder()).toList()) {
            Files.deleteIfExists(f);
          }
        }
      }
    }

    static final java.util.regex.Pattern TIMESTAMP =
        java.util.regex.Pattern.compile(
            "\"date\": \"\\d{4}-\\d{2}-\\d{2}T[0-9:.]+[+-]\\d{2}:\\d{2}\"");

    static final java.util.regex.Pattern LOG_PREFIX =
        java.util.regex.Pattern.compile("^\\d{4}-\\d{2}-\\d{2} \\d{2}:\\d{2}:\\d{2}\\.\\d{3} ");
    static final java.util.regex.Pattern JOB_ID =
        java.util.regex.Pattern.compile("\\[[0-9A-F]{6}\\\\[0-9A-F]{6}\\]");

    static String normalizeStageLine(String line) {
      return line.replaceAll("[0-9]+(\\.[0-9]+)?", "<t>");
    }
  }
}
