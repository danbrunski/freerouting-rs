import app.freerouting.Freerouting;
import app.freerouting.board.facade.RoutingBoard;
import app.freerouting.core.RoutingJob;
import app.freerouting.core.scoring.BoardStatistics;
import app.freerouting.io.specctra.SesReader;
import app.freerouting.management.BoardLoader;
import app.freerouting.settings.GlobalSettings;
import app.freerouting.settings.OptimizerScoringVersion;
import app.freerouting.settings.RouterScoringVersion;
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
import java.util.UUID;
import java.util.function.Consumer;

/**
 * Board probe {@code scores} (B06.3): the router and optimizer scores of {@code new
 * BoardStatistics(board)} for a set of {@code RouterSettings}. Per stage ({@code load}, then
 * {@code ses} when the row has a session) one line with the statistics the scores read, then one
 * line per settings variant with {@code getRouterScore}, {@code getOptimizerScore} and {@code
 * getOptimizerRankingScore} (floats widened to double, as raw bits; a NaN as {@code Double.NaN}).
 *
 * <p>The variants: {@code null}; {@code defaults} ({@code DefaultSettings} merged as {@code
 * SettingsMerger} does, V2); {@code v1} (both versions {@code V1_LEGACY}); one V2 variant per
 * router and optimizer score weight at twice its default ({@code router.<field>}, {@code
 * optimizer.<field>}); one legacy variant per legacy cost at twice its default ({@code
 * v1.<field>}); and {@code optimizer.lengthFloor=0}, where a board without traces divides zero by
 * zero.
 *
 * <pre>
 * tools/java-reference/probe.sh &lt;pin&gt; tools/java-reference/probes/board/ScoresProbe.java \
 *     board.dsn board.ses|- out.jsonl
 * </pre>
 */
public class ScoresProbe {
  public static void main(String[] args) throws Exception {
    Boards.Out out = new Boards.Out("scores", args[0]);
    try {
      RoutingBoard board = Boards.load(args[0]);
      stage(out, "load", board);
      if (!args[1].equals("-")) {
        Boards.importSession(board, args[1]);
        stage(out, "ses", board);
      }
    } catch (Throwable t) {
      out.exception(t);
    }
    out.write(args[2]);
  }

  static void stage(Boards.Out out, String name, RoutingBoard board) {
    BoardStatistics s = new BoardStatistics(board);
    out.line(
        new Boards.Line()
            .str("stage", name)
            .num("maximum", s.connections.maximumCount)
            .num("incomplete", s.connections.incompleteCount)
            .num("violations", s.clearanceViolations.totalCount)
            .dbl("violationUm", s.clearanceViolations.totalViolationUm)
            .dbl("difficulty", s.difficulty.difficultyD)
            .num("bends", s.bends.totalCount)
            .num("vias", s.vias.totalCount)
            .dbl("lengthMm", s.traces.totalLengthMm)
            .dbl("minLengthMm", s.bounds.minTraceLengthMm)
            .num("minVias", s.bounds.minViaCount)
            .num("minBends", s.bounds.minBendCount)
            .end());
    for (Variant v : variants()) {
      RouterSettings settings = v.settings();
      out.line(
          new Boards.Line()
              .str("variant", v.name)
              .dbl("router", canonical(s.getRouterScore(settings)))
              .dbl("optimizer", canonical(s.getOptimizerScore(settings)))
              .dbl("ranking", canonical(s.getOptimizerRankingScore(settings)))
              .end());
    }
  }

  /** A named change of the default settings; {@code null} change means {@code null} settings. */
  record Variant(String name, Consumer<RouterSettings> change) {
    RouterSettings settings() {
      if (change == null) {
        return null;
      }
      SettingsMerger merger = new SettingsMerger();
      merger.addOrReplaceSources(new DefaultSettings());
      RouterSettings settings = merger.merge();
      change.accept(settings);
      return settings;
    }
  }

  static List<Variant> variants() {
    List<Variant> v = new ArrayList<>();
    v.add(new Variant("null", null));
    v.add(new Variant("defaults", s -> {}));
    v.add(new Variant("v1", ScoresProbe::legacy));
    v.add(new Variant("router.unroutedConnectionWeight", s -> s.routerScoring.unroutedConnectionWeight *= 2));
    v.add(new Variant("router.unroutedFreeFraction", s -> s.routerScoring.unroutedFreeFraction *= 2));
    v.add(new Variant("router.unroutedFirstHalfWeight", s -> s.routerScoring.unroutedFirstHalfWeight *= 2));
    v.add(new Variant("router.unroutedSecondHalfWeight", s -> s.routerScoring.unroutedSecondHalfWeight *= 2));
    v.add(new Variant("router.clearanceViolationCountWeight", s -> s.routerScoring.clearanceViolationCountWeight *= 2));
    v.add(new Variant("router.clearanceViolationDepthWeight", s -> s.routerScoring.clearanceViolationDepthWeight *= 2));
    v.add(new Variant("router.clearanceViolationDepthScale", s -> s.routerScoring.clearanceViolationDepthScale *= 2));
    v.add(new Variant("optimizer.excessWireLengthWeight", s -> s.optimizerScoring.excessWireLengthWeight *= 2));
    v.add(new Variant("optimizer.excessViaWeight", s -> s.optimizerScoring.excessViaWeight *= 2));
    v.add(new Variant("optimizer.excessBendWeight", s -> s.optimizerScoring.excessBendWeight *= 2));
    v.add(new Variant("optimizer.lengthFloor", s -> s.optimizerScoring.lengthFloor *= 2));
    v.add(new Variant("optimizer.difficultyScaleFloor", s -> s.optimizerScoring.difficultyScaleFloor *= 2));
    v.add(new Variant("v1.bendPenalty", s -> { legacy(s); s.scoring.bendPenalty *= 2; }));
    v.add(new Variant("v1.viaCosts", s -> { legacy(s); s.scoring.viaCosts *= 2; }));
    v.add(new Variant("v1.defaultPreferredDirectionTraceCost", s -> { legacy(s); s.scoring.defaultPreferredDirectionTraceCost *= 2; }));
    v.add(new Variant("optimizer.lengthFloor=0", s -> s.optimizerScoring.lengthFloor = 0f));
    return v;
  }

  /**
   * A score widened to double, with every NaN written as {@code Double.NaN}: the sign and payload
   * of a NaN from {@code 0.0 / 0.0} depend on the processor (x86 sets the sign bit).
   */
  static double canonical(float score) {
    return Float.isNaN(score) ? Double.NaN : score;
  }

  static void legacy(RouterSettings s) {
    s.routerScoring.version = RouterScoringVersion.V1_LEGACY;
    s.optimizerScoring.version = OptimizerScoringVersion.V1_LEGACY;
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
