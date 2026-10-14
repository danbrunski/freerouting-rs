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
import java.util.stream.Stream;

/**
 * Dumps reference outputs from the Java Freerouting engine for every DSN fixture, so the Rust
 * port can be compared against them.
 *
 * <pre>
 * ReferenceDump load  FIXTURES_DIR OUT_DIR
 * ReferenceDump route FIXTURES_DIR OUT_DIR [MAX_PASSES] [TIMEOUT hh:mm:ss] [NAME_FILTER]
 * </pre>
 *
 * <p>{@code load} parses each board with {@code DsnReader} and writes its {@code BoardStatistics}
 * (item counts, nets, incompletes, clearance violations). {@code route} runs the batch autorouter
 * the way the CLI does and writes the SES output plus statistics and score after routing.
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

    List<Path> boards;
    try (Stream<Path> files = Files.walk(fixtures)) {
      boards =
          files
              .filter(p -> p.getFileName().toString().toLowerCase().endsWith(".dsn"))
              .sorted()
              .toList();
    }
    String filter = args.length > 5 ? args[5] : null;

    for (Path board : boards) {
      String rel = fixtures.relativize(board).toString().replace('\\', '/');
      if (filter != null && !rel.contains(filter)) {
        continue;
      }
      System.out.println(mode + ": " + rel);
      JsonObject result;
      try {
        result =
            switch (mode) {
              case "load" -> load(board);
              case "route" ->
                  route(
                      board,
                      out.resolve("route").resolve(rel.replaceAll("\\.dsn$", ".ses")),
                      args.length > 3 ? Integer.parseInt(args[3]) : 100,
                      args.length > 4 ? args[4] : "00:02:00");
              default -> throw new IllegalArgumentException("unknown mode " + mode);
            };
      } catch (Throwable t) {
        result = new JsonObject();
        result.addProperty("result", "exception");
        result.addProperty("error", t.toString());
      }
      result.addProperty("fixture", rel);
      Path target = out.resolve(mode).resolve(rel.replaceAll("\\.dsn$", ".json"));
      Files.createDirectories(target.getParent());
      Files.writeString(target, GSON.toJson(result) + "\n", StandardCharsets.UTF_8);
    }
    System.exit(0);
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

  private static JsonObject route(Path board, Path sesOut, int maxPasses, String timeout)
      throws Exception {
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
    merger.addOrReplaceSources(new Overrides(maxPasses, timeout));
    job.routerSettings = merger.merge();

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
    result.addProperty("routeMillis", (System.nanoTime() - start) / 1_000_000);
    result.addProperty("result", job.state.toString().toLowerCase());
    result.addProperty("maxPasses", maxPasses);
    result.addProperty("timeout", timeout);
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
    synchronized (scheduler.jobs) {
      scheduler.jobs.clear();
    }
    return result;
  }

  private static JsonElement statistics(BoardStatistics stats) {
    return JsonParser.parseString(stats.toString());
  }

  /** Highest-priority settings source carrying the harness's pass and time limits. */
  private static final class Overrides implements SettingsSource {
    private final RouterSettings settings = new RouterSettings();

    Overrides(int maxPasses, String timeout) {
      settings.optimizer = null;
      settings.scoring = null;
      settings.autorouter.maxPasses = maxPasses;
      settings.jobTimeoutString = timeout;
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
}
