import app.freerouting.Freerouting;
import app.freerouting.board.actions.ItemInfoPrinter;
import app.freerouting.board.facade.RoutingBoard;
import app.freerouting.board.model.items.Item;
import app.freerouting.board.model.structure.FixedState;
import app.freerouting.core.RoutingJob;
import app.freerouting.geometry.planar.FloatPoint;
import app.freerouting.geometry.planar.Shape;
import app.freerouting.io.specctra.SesReader;
import app.freerouting.management.BoardLoader;
import app.freerouting.settings.GlobalSettings;
import app.freerouting.util.TextManager;
import java.io.FileInputStream;
import java.io.InputStream;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.Collection;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Locale;
import java.util.Map;
import java.util.Set;
import java.util.UUID;

/**
 * Board probe {@code item-info} (B08.2): what {@code Item.printInfo} prints, as data. The board's
 * items are grouped by Java class in item-list order (highest id first); per class the items at
 * positions 0, n/3, 2n/3 and n-1, and the first three that have clearance violations, are printed
 * (with {@code Locale.ENGLISH}) into a recording {@link ItemInfoPrinter}. One line per printer
 * call, {@code stage:"info"}, keys {@code item}, {@code seq}, {@code op}:
 *
 * <ul>
 *   <li>{@code head}: the Java simple class name, before the first printer call;
 *   <li>{@code link}: {@code append(linkName, title, object)}: the link text and the class of the
 *       linked object ({@code Component}, {@code Padstack}, {@code Row} for a clearance matrix
 *       row, {@code Net});
 *   <li>{@code point}: {@code append(FloatPoint)} as two raw doubles, untransformed;
 *   <li>{@code number}: {@code append(double)} as raw bits, untransformed;
 *   <li>{@code text}: a plain {@code append(String)} that follows a plain append ending in a
 *       space, which is how every printed value (pin name, layer name, hole count) follows its
 *       label; the labels themselves are localized text and are skipped;
 *   <li>{@code fixed}: the same, when the text is that of a {@code FixedState}; its enum name;
 *   <li>{@code items}: {@code appendItems}: the count and the ids in iteration order;
 *   <li>{@code objects}: {@code appendObjects}: the count;
 *   <li>{@code newline}.
 * </ul>
 *
 * Runs on the boards in {@link #BOARDS} (with the session imported where the row has one) and
 * exits 3 on the others.
 *
 * <pre>
 * tools/java-reference/probe.sh &lt;pin&gt; tools/java-reference/probes/board/ItemInfoProbe.java \
 *     board.dsn board.ses|- out.jsonl
 * </pre>
 */
public class ItemInfoProbe {
  static final Set<String> BOARDS = Set.of("Issue026-J2_reference", "Issue145-smoothieboard", "Issue039-bug-design");

  public static void main(String[] args) throws Exception {
    if (!BOARDS.contains(Boards.board(args[0]))) {
      System.exit(3);
    }
    Boards.Out out = new Boards.Out("item-info", args[0]);
    try {
      RoutingBoard board = Boards.load(args[0]);
      if (!args[1].equals("-")) {
        Boards.importSession(board, args[1]);
      }
      Map<String, List<Item>> byClass = new LinkedHashMap<>();
      for (Item item : board.getItems()) {
        byClass.computeIfAbsent(item.getClass().getSimpleName(), k -> new ArrayList<>()).add(item);
      }
      List<String> classes = new ArrayList<>(byClass.keySet());
      java.util.Collections.sort(classes);
      for (String name : classes) {
        List<Item> items = byClass.get(name);
        int n = items.size();
        Set<Item> chosen = new java.util.LinkedHashSet<>();
        for (int index : new int[] {0, n / 3, 2 * n / 3, n - 1}) {
          chosen.add(items.get(index));
        }
        int withViolations = 0;
        for (Item item : items) {
          if (withViolations < 3 && !item.clearanceViolations().isEmpty()) {
            chosen.add(item);
            withViolations++;
          }
        }
        for (Item item : chosen) {
          Recorder recorder = new Recorder(out, item);
          recorder.head();
          item.printInfo(recorder, Locale.ENGLISH);
        }
      }
    } catch (Throwable t) {
      out.exception(t);
    }
    out.write(args[2]);
  }

  /** An {@code ItemInfoPrinter} that writes each call as a line. */
  static final class Recorder implements ItemInfoPrinter {
    private final Boards.Out out;
    private final Item item;
    private final TextManager tm;
    private int seq = 0;
    private boolean afterLabel = false;

    Recorder(Boards.Out out, Item item) {
      this.out = out;
      this.item = item;
      this.tm = new TextManager(item.getClass(), Locale.ENGLISH);
    }

    /** The first line of an item: a {@code ComponentOutline} prints nothing else. */
    void head() {
      out.line(line("head").str("class", item.getClass().getSimpleName()).end());
    }

    private Boards.Line line(String op) {
      return new Boards.Line()
          .str("stage", "info")
          .num("item", item.getId())
          .num("seq", seq++)
          .str("op", op);
    }

    @Override
    public boolean append(String string) {
      boolean data = afterLabel;
      afterLabel = string.endsWith(" ");
      if (!data) {
        return true;
      }
      for (FixedState state : FixedState.values()) {
        if (state != FixedState.UNFIXED && string.equals(tm.getText(state.toString()))) {
          out.line(line("fixed").str("value", state.name()).end());
          return true;
        }
      }
      out.line(line("text").str("value", string).end());
      return true;
    }

    @Override
    public boolean append(String linkName, String windowTitle, ItemInfoPrinter.Printable object) {
      afterLabel = false;
      String type = object.getClass().getSimpleName();
      out.line(line("link").str("name", linkName).str("object", type).end());
      return true;
    }

    @Override
    public boolean append(double value) {
      afterLabel = false;
      out.line(line("number").dbl("value", value).end());
      return true;
    }

    @Override
    public boolean append(FloatPoint point) {
      afterLabel = false;
      out.line(line("point").dbl("x", point.x).dbl("y", point.y).end());
      return true;
    }

    @Override
    public boolean append(Shape shape, Locale locale) {
      afterLabel = false;
      throw new IllegalStateException("append(Shape) is not used by the items");
    }

    @Override
    public boolean appendWithoutTransforming(double value) {
      afterLabel = false;
      out.line(line("number").dbl("value", value).end());
      return true;
    }

    @Override
    public boolean appendBold(String string) {
      afterLabel = false;
      return true;
    }

    @Override
    public boolean newline() {
      afterLabel = false;
      out.line(line("newline").end());
      return true;
    }

    @Override
    public boolean indent() {
      afterLabel = false;
      return true;
    }

    @Override
    public boolean appendItems(String linkName, String windowTitle, Collection<Item> items) {
      afterLabel = false;
      StringBuilder ids = new StringBuilder("[");
      for (Item contact : items) {
        if (ids.length() > 1) {
          ids.append(',');
        }
        ids.append(contact.getId());
      }
      ids.append(']');
      out.line(line("items").num("count", items.size()).raw("ids", ids.toString()).end());
      return true;
    }

    @Override
    public boolean appendObjects(
        String buttonName, String windowTitle, Collection<ItemInfoPrinter.Printable> objects) {
      afterLabel = false;
      out.line(line("objects").num("count", objects.size()).end());
      return true;
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
