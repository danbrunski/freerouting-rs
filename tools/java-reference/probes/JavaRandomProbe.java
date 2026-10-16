import java.util.ArrayList;
import java.util.Collections;
import java.util.List;
import java.util.Random;

/**
 * The sample probe for tools/java-reference/probe.sh. Prints what freerouting_geometry::jmath
 * asserts for Java's Random: {@code new Random(42).nextInt(100)} five times on one line, then
 * {@code Collections.shuffle(0..9, new Random(99))}.
 *
 * <pre>
 * tools/java-reference/probe.sh &lt;pin-checkout&gt; tools/java-reference/probes/JavaRandomProbe.java
 * 30 63 48 84 70
 * [8, 1, 4, 3, 9, 6, 0, 2, 5, 7]
 * </pre>
 */
public class JavaRandomProbe {
  public static void main(String[] args) {
    Random random = new Random(42);
    StringBuilder line = new StringBuilder();
    for (int i = 0; i < 5; i++) {
      if (i > 0) {
        line.append(' ');
      }
      line.append(random.nextInt(100));
    }
    System.out.println(line);

    List<Integer> list = new ArrayList<>();
    for (int i = 0; i < 10; i++) {
      list.add(i);
    }
    Collections.shuffle(list, new Random(99));
    System.out.println(list);
  }
}
