package app.freerouting.autoroute.pipeline;

import app.freerouting.Freerouting;
import app.freerouting.board.facade.RoutingBoard;
import app.freerouting.board.model.items.Item;
import app.freerouting.board.model.items.Pin;
import app.freerouting.core.RoutingJob;
import app.freerouting.core.StoppableThread;
import app.freerouting.drc.DesignRulesChecker;
import app.freerouting.geometry.planar.FloatPoint;
import app.freerouting.io.specctra.SesReader;
import app.freerouting.management.BoardLoader;
import app.freerouting.settings.GlobalSettings;
import app.freerouting.settings.RouterSettings;
import java.io.FileInputStream;
import java.io.InputStream;
import java.lang.reflect.Constructor;
import java.lang.reflect.Field;
import java.lang.reflect.Method;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.Collection;
import java.util.Collections;
import java.util.List;
import java.util.Random;
import java.util.Set;
import java.util.TreeSet;
import java.util.UUID;

/**
 * The batch-pipeline probe (A10.4, A10.6, A10.7): the Java side of
 * crates/freerouting/tests/autoroute_batch_control.rs. In package app.freerouting.autoroute.pipeline
 * so it calls the package-private methods directly; private state is read by reflection.
 *
 * <pre>
 * probe.sh PIN BatchProbe.java items FIXTURE.dsn OUT.jsonl
 * probe.sh PIN BatchProbe.java fanout-order FIXTURE.dsn OUT.jsonl
 * probe.sh PIN BatchProbe.java unrouted-report FIXTURE.dsn OUT.jsonl
 * probe.sh PIN BatchProbe.java shuffle SEED COUNT      (prints the shuffled 0..COUNT-1)
 * </pre>
 *
 * Kinds:
 * - items: getAutorouteItems ids and the plane-list length, then reorderSingleThreadItems at
 *   pass 2 with no history, at pass 2 with the load-time incomplete nets as the previous
 *   pass's, and at passes 2 and 3 with stagnationCount forced to 2 (the shuffle).
 * - fanout-order: the components and pins of BatchFanout's constructor (sortedComponents) for
 *   each pin sorting order, with the values the comparators read.
 * - unrouted-report: AutorouteUnroutedReport.build on the loaded board.
 */
public class BatchProbe {

  public static void main(String[] args) throws Exception {
    String kind = args[0];
    if (kind.equals("shuffle")) {
      List<Integer> list = new ArrayList<>();
      for (int i = 0; i < Integer.parseInt(args[2]); i++) {
        list.add(i);
      }
      Collections.shuffle(list, new Random(Long.parseLong(args[1])));
      System.out.println(list);
      return;
    }
    String dsn = args[1];
    Boards.Out out = new Boards.Out(kind, dsn);
    try {
      RoutingJob job = loadJob(dsn);
      switch (kind) {
        case "items" -> items(out, job);
        case "fanout-order" -> fanoutOrder(out, job);
        case "unrouted-report" ->
            out.line(
                new Boards.Line()
                    .str("stage", "report")
                    .str("text", AutorouteUnroutedReport.build(job.board))
                    .end());
        default -> throw new IllegalArgumentException("unknown kind " + kind);
      }
    } catch (Throwable t) {
      out.exception(t);
    }
    out.write(args[2]);
  }

  /** Boards.load, keeping the job. */
  static RoutingJob loadJob(String dsn) throws Exception {
    Freerouting.globalSettings = new GlobalSettings();
    RoutingJob job = new RoutingJob(new UUID(0L, 1L));
    job.setInput(dsn);
    if (!BoardLoader.loadBoardIfNeeded(job)) {
      throw new IllegalStateException("BoardLoader.loadBoardIfNeeded failed");
    }
    job.board.awaitPostLoad();
    return job;
  }

  static Object field(Object owner, String name) throws Exception {
    return declared(owner.getClass(), name).get(owner);
  }

  static void setField(Object owner, String name, Object value) throws Exception {
    declared(owner.getClass(), name).set(owner, value);
  }

  static Field declared(Class<?> type, String name) throws Exception {
    for (Class<?> c = type; c != null; c = c.getSuperclass()) {
      try {
        Field f = c.getDeclaredField(name);
        f.setAccessible(true);
        return f;
      } catch (NoSuchFieldException e) {
        // try the superclass
      }
    }
    throw new NoSuchFieldException(name);
  }

  static String ids(Collection<Item> items) {
    StringBuilder text = new StringBuilder("[");
    for (Item item : items) {
      if (text.length() > 1) {
        text.append(',');
      }
      text.append(item.getId());
    }
    return text.append(']').toString();
  }

  static String ints(Collection<Integer> values) {
    StringBuilder text = new StringBuilder("[");
    for (int value : values) {
      if (text.length() > 1) {
        text.append(',');
      }
      text.append(value);
    }
    return text.append(']').toString();
  }

  static void items(Boards.Out out, RoutingJob job) throws Exception {
    RoutingBoard board = job.board;
    BatchAutorouter router = new BatchAutorouter(job);
    List<Item> selected = new ArrayList<>(router.getAutorouteItems(board));
    List<?> planeList = (List<?>) field(router, "reusablePlaneItemList");
    out.line(
        new Boards.Line()
            .str("stage", "select")
            .num("plane", planeList.size())
            .raw("items", ids(selected))
            .end());

    DesignRulesChecker drc = new DesignRulesChecker(board, null);
    drc.calculateAllIncompletes();
    Set<Integer> incompleteNets = new TreeSet<>();
    for (int net = 1; net <= board.rules.nets.maxNetNumber(); net++) {
      if (drc.getIncompleteCount(net) > 0) {
        incompleteNets.add(net);
      }
    }

    Object runner = field(router, "passRunner");
    Method reorder =
        runner.getClass().getDeclaredMethod("reorderSingleThreadItems", List.class, int.class);
    reorder.setAccessible(true);
    Object[][] variants = {
      {"pass2-sorted", 2, 0, Collections.<Integer>emptySet()},
      {"pass2-persistent", 2, 0, incompleteNets},
      {"pass2-shuffled", 2, 2, Collections.<Integer>emptySet()},
      {"pass3-shuffled", 3, 2, Collections.<Integer>emptySet()},
    };
    for (Object[] variant : variants) {
      setField(runner, "stagnationCount", variant[2]);
      setField(runner, "previousIncompleteNets", variant[3]);
      List<Item> order = new ArrayList<>(selected);
      reorder.invoke(runner, order, variant[1]);
      @SuppressWarnings("unchecked")
      Set<Integer> nets = (Set<Integer>) variant[3];
      out.line(
          new Boards.Line()
              .str("stage", (String) variant[0])
              .num("pass", (Integer) variant[1])
              .num("stagnation", (Integer) variant[2])
              .raw("nets", ints(nets))
              .num("maxNet", board.rules.nets.maxNetNumber())
              .raw("items", ids(order))
              .end());
    }
  }

  static void fanoutOrder(Boards.Out out, RoutingJob job) throws Exception {
    RoutingBoard board = job.board;
    String[] orders = {
      "outer_first", "inner_first", "distanceToClosestOnNet", "surroundingsDensity",
      "distance_to_closest_on_net"
    };
    Constructor<BatchFanout> make =
        BatchFanout.class.getDeclaredConstructor(
            RoutingBoard.class, RouterSettings.class, StoppableThread.class);
    make.setAccessible(true);
    for (String order : orders) {
      RouterSettings settings = job.routerSettings;
      settings.fanout.pinSortingOrder = order;
      BatchFanout fanout = make.newInstance(board, settings, null);
      for (Object component : (Collection<?>) field(fanout, "sortedComponents")) {
        app.freerouting.board.model.structure.Component boardComponent =
            (app.freerouting.board.model.structure.Component) field(component, "boardComponent");
        FloatPoint centre = (FloatPoint) field(component, "gravityCenterOfSmdPins");
        StringBuilder pins = new StringBuilder("[");
        StringBuilder dist = new StringBuilder("[");
        StringBuilder closest = new StringBuilder("[");
        StringBuilder density = new StringBuilder("[");
        for (Object pin : (Collection<?>) field(component, "smdPins")) {
          if (pins.length() > 1) {
            pins.append(',');
            dist.append(',');
            closest.append(',');
            density.append(',');
          }
          Pin boardPin = (Pin) field(pin, "boardPin");
          pins.append(boardPin.getId());
          dist.append('"').append(Boards.bits((Double) field(pin, "distanceToComponentCenter")));
          dist.append('"');
          closest.append('"').append(Boards.bits((Double) field(pin, "distanceToClosestOnNet")));
          closest.append('"');
          density.append(field(pin, "surroundingsDensity"));
        }
        out.line(
            new Boards.Line()
                .str("stage", order)
                .num("component", boardComponent.id)
                .num("smdPins", (Integer) field(component, "smdPinCount"))
                .dbl("cx", centre.x)
                .dbl("cy", centre.y)
                .raw("pins", pins.append(']').toString())
                .raw("dist", dist.append(']').toString())
                .raw("closest", closest.append(']').toString())
                .raw("density", density.append(']').toString())
                .end());
      }
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
