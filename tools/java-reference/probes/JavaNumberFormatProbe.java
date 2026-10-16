import app.freerouting.datastructures.IdentifierType;
import java.io.ByteArrayOutputStream;
import java.io.OutputStreamWriter;
import java.math.BigDecimal;
import java.nio.charset.StandardCharsets;

/**
 * Prints what the SES writer's number and name formatting does at the pin (C05.1), as
 * tab-separated lines the Rust tests paste:
 *
 * <pre>
 * D  &lt;raw bits, hex&gt;  &lt;Double.toString&gt;  &lt;BigDecimal.valueOf(d).stripTrailingZeros().toPlainString()&gt;
 * I  &lt;name, \\u-escaped&gt;  &lt;IdentifierType.write output with the SES writer's reserved characters, \\u-escaped&gt;
 * </pre>
 *
 * <pre>
 * tools/java-reference/probe.sh &lt;pin-checkout&gt; tools/java-reference/probes/JavaNumberFormatProbe.java
 * </pre>
 */
public class JavaNumberFormatProbe {
  private static final String[] DOUBLES = {
    "0.0", "-0.0", "1.0", "-1.0", "10.0", "100.0", "339.0", "338.5", "180.0", "0.5", "-45.25",
    "12.2501234567", "0.001", "0.0009999999999999998", "1.0E-4", "1.0E-5", "123456.7", "999999.9",
    "9999999.0", "9999999.999999998", "1.0E7", "1.2345E8", "1.7145E7", "1.9684999999999996E7",
    "-533400.0", "939800.0", "2540000.0", "1.0E23", "9.999999999999999E22", "1.0E-10",
    "4.9E-324", "1.0E-323", "2.2250738585072014E-308", "2.225073858507201E-308",
    "1.7976931348623157E308", "3.0E-5", "0.1", "0.2", "0.30000000000000004", "1.0E21", "123456789.0",
    "4.35", "2.675", "5.0E-324", "7.0E-323", "1.5E-323", "100000.0", "0.0010000000000000002",
    "8.41E21", "1.0E22", "9007199254740993.0", "64.0", "338.50000001", "1.0E-3", "5.0E-4"
  };

  private static final String[] NAMES = {
    "600", "-600", "test", "test-with-reserved", "600a", "", "a b", "a(b", "a)b", "a;b", "a_b", "a/b",
    "a~b", "a{b", "a}b", "-", "-a", "--1", "1", "-1x", "F.Cu", "+5V", "GND", "été",
    "Кир", "a\"b", "\"quoted\"", "\"ab\"", "\"abc\"", "\"\"quoted\"\"", "x\"", "\"x",
    "1\nabc", "1abc\n", "µ", "\"a b\"", "'q'", "a'b", "\u0000", "(1)", "v1.0", "0", "-.5"
  };

  public static void main(String[] args) throws Exception {
    for (String text : DOUBLES) {
      double d = Double.parseDouble(text);
      System.out.println(
          "D\t"
              + Long.toHexString(Double.doubleToRawLongBits(d))
              + "\t"
              + Double.toString(d)
              + "\t"
              + BigDecimal.valueOf(d).stripTrailingZeros().toPlainString());
    }
    String[] reservedChars = {"(", ")", " ", ";", "-", "_", "/", "~", "{", "}"};
    IdentifierType identifierType = new IdentifierType(reservedChars, "\"");
    for (String name : NAMES) {
      ByteArrayOutputStream baos = new ByteArrayOutputStream();
      OutputStreamWriter writer = new OutputStreamWriter(baos, StandardCharsets.UTF_8);
      identifierType.write(name, writer);
      writer.flush();
      System.out.println(
          "I\t" + escape(name) + "\t" + escape(baos.toString(StandardCharsets.UTF_8)));
    }
    IdentifierType single = new IdentifierType(reservedChars, "'");
    for (String name : NAMES) {
      ByteArrayOutputStream baos = new ByteArrayOutputStream();
      OutputStreamWriter writer = new OutputStreamWriter(baos, StandardCharsets.UTF_8);
      single.write(name, writer);
      writer.flush();
      System.out.println(
          "S\t" + escape(name) + "\t" + escape(baos.toString(StandardCharsets.UTF_8)));
    }
  }

  private static String escape(String s) {
    StringBuilder out = new StringBuilder();
    for (char c : s.toCharArray()) {
      if (c < 0x20 || c > 0x7e || c == '\\') {
        out.append(String.format("\\u%04x", (int) c));
      } else {
        out.append(c);
      }
    }
    return out.toString();
  }
}
