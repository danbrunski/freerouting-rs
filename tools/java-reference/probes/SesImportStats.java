import app.freerouting.board.actions.ItemIdGenerator;
import app.freerouting.board.facade.BasicBoard;
import app.freerouting.core.scoring.BoardStatistics;
import app.freerouting.io.BoardReadResult;
import app.freerouting.io.specctra.DsnReader;
import app.freerouting.io.specctra.SesReader;
import java.io.FileInputStream;
import java.io.InputStream;

/**
 * Loads a DSN board, imports a SES session into it (Java's SesReader), and prints the import
 * summary and the BoardStatistics fields that {@code freerouting-dev ses-diff} compares.
 *
 * <pre>
 * tools/java-reference/probe.sh &lt;pin-checkout&gt; tools/java-reference/probes/SesImportStats.java board.dsn board.ses
 * </pre>
 */
public class SesImportStats {
  public static void main(String[] args) throws Exception {
    BoardReadResult read;
    try (InputStream in = new FileInputStream(args[0])) {
      read = DsnReader.readBoard(in, null, new ItemIdGenerator(), args[0]);
    }
    BasicBoard board = switch (read) {
      case BoardReadResult.Success s -> s.board();
      case BoardReadResult.OutlineMissing s -> s.board();
      default -> throw new IllegalStateException(read.toString());
    };
    board.expandBoundingBoxToIncludeAllItems();
    try (InputStream in = new FileInputStream(args[1])) {
      System.out.println(SesReader.read(in, board));
    }
    BoardStatistics s = new BoardStatistics(board);
    System.out.println("incomplete " + s.connections.incompleteCount);
    System.out.println("violations " + s.clearanceViolations.totalCount);
    System.out.println("vias " + s.vias.totalCount);
    System.out.println("length_mm " + s.traces.totalLengthMm);
    System.out.println("bends " + s.bends.totalCount);
    System.out.println("traces " + s.traces.totalCount);
  }
}
