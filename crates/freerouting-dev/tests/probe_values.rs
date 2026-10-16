//! The values `tools/java-reference/probes/JavaRandomProbe.java` prints at
//! the pin (F02.1), checked against the Rust `java.util.Random` port.
//!
//! `tools/java-reference/probe.sh <pin> tools/java-reference/probes/JavaRandomProbe.java`
//! printed `30 63 48 84 70` and `[8, 1, 4, 3, 9, 6, 0, 2, 5, 7]` at `cda540f`.

use freerouting::geometry::jmath::JavaRandom;

#[test]
fn java_random_probe_values_match_jmath() {
    let mut random = JavaRandom::new(42);
    let ints: Vec<i32> = (0..5).map(|_| random.next_int(100)).collect();
    assert_eq!(ints, [30, 63, 48, 84, 70]);
    let mut list: Vec<i32> = (0..10).collect();
    JavaRandom::new(99).shuffle(&mut list);
    assert_eq!(list, [8, 1, 4, 3, 9, 6, 0, 2, 5, 7]);
}
