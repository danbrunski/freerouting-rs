import app.freerouting.Freerouting;
import app.freerouting.board.facade.RoutingBoard;
import app.freerouting.board.model.items.Trace;
import app.freerouting.core.RoutingJob;
import app.freerouting.core.scoring.BoardStatistics;
import app.freerouting.io.specctra.SesReader;
import app.freerouting.management.BoardLoader;
import app.freerouting.settings.GlobalSettings;
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
 * Board probe {@code state-hash} (B01.5): which board states Java's {@code getHash} considers
 * equal. Steps: load (h0); {@code generateSnapshot} (h1); import the session (h2); {@code
 * generateSnapshot}, then unfix and remove the 5 highest-id traces (h3); undo (h4); redo (h5); {@code
 * deepCopy}, hashing the copy (h6); {@code new BoardStatistics} on the copy (h7). Each line names
 * the earlier steps whose hash equals this one's. Runs on the boards in {@link #BOARDS} only and
 * exits 3 on the others.
 *
 * <pre>
 * tools/java-reference/probe.sh &lt;pin&gt; tools/java-reference/probes/board/StateHashProbe.java \
 *     board.dsn board.ses out.jsonl
 * </pre>
 */
public class StateHashProbe {
  static final Set<String> BOARDS =
      Set.of("Issue026-J2_reference", "Issue508-DAC2020_bm08", "Issue413-test");

  public static void main(String[] args) throws Exception {
    if (!BOARDS.contains(Boards.board(args[0])) || args[1].equals("-")) {
      System.exit(3);
    }
    Boards.Out out = new Boards.Out("state-hash", args[0]);
    List<String> hashes = new ArrayList<>();
    try {
      RoutingBoard board = Boards.load(args[0]);
      step(out, hashes, "h0", board);
      board.generateSnapshot();
      step(out, hashes, "h1", board);
      Boards.importSession(board, args[1]);
      step(out, hashes, "h2", board);
      board.generateSnapshot();
      List<Trace> traces = new ArrayList<>(board.getTraces());
      for (Trace trace : traces.subList(0, Math.min(5, traces.size()))) {
        // The session's wires are user fixed, which removeItem refuses.
        trace.unfix();
        board.removeItem(trace);
      }
      step(out, hashes, "h3", board);
      board.undo(null);
      step(out, hashes, "h4", board);
      board.redo(null);
      step(out, hashes, "h5", board);
      RoutingBoard copy = board.deepCopy();
      step(out, hashes, "h6", copy);
      new BoardStatistics(copy);
      step(out, hashes, "h7", copy);
    } catch (Throwable t) {
      out.exception(t);
    }
    out.write(args[2]);
  }

  static void step(Boards.Out out, List<String> hashes, String name, RoutingBoard board) {
    String hash = board.getHash();
    StringBuilder equals = new StringBuilder("[");
    for (int i = 0; i < hashes.size(); i++) {
      if (hashes.get(i).equals(hash)) {
        if (equals.length() > 1) {
          equals.append(',');
        }
        equals.append(i);
      }
    }
    equals.append(']');
    hashes.add(hash);
    out.line(
        new Boards.Line()
            .str("stage", name)
            .num("maxGeneratedId", board.communication.idGenerator.maxGeneratedId())
            .raw("equals", equals.toString())
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
