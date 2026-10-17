//! K08.1 to K08.3: the copper index, the diff, the splice and the UUIDs on a
//! small hand-written board, in a 100 nm grid frame (the engine's frame is
//! tested in `crates/freerouting/tests/kicad_write.rs`).

use freerouting_kicad::read;
use freerouting_kicad::write::{
    BoardFrame, CopperIndex, CopperKind, FinalItem, FinalTrace, FinalVia, GridFrame, NewItem,
    UuidGen, diff, fnv1a128, render, render_with,
};

const BOARD: &str = r#"(kicad_pcb
	(version 20260206)
	(generator "pcbnew")
	(generator_version "10.0")
	(layers
		(0 "F.Cu" signal)
		(2 "B.Cu" signal)
		(25 "Edge.Cuts" user)
	)
	(gr_rect
		(start 0 0)
		(end 50 50)
		(stroke
			(width 0.1)
			(type default)
		)
		(fill no)
		(layer "Edge.Cuts")
		(uuid "00000000-0000-4000-8000-000000000001")
	)
	(segment
		(start 10 10)
		(end 20 10)
		(width 0.25)
		(layer "F.Cu")
		(net "A")
		(uuid "00000000-0000-4000-8000-000000000002")
	)
	(segment
		(start 20 10)
		(end 20 20)
		(width 0.25)
		(layer "F.Cu")
		(net "A")
		(uuid "00000000-0000-4000-8000-000000000003")
	)
	(via
		(at 20 20)
		(size 0.8)
		(drill 0.4)
		(layers "F.Cu" "B.Cu")
		(net "A")
		(uuid "00000000-0000-4000-8000-000000000004")
	)
	(segment
		(start 20 20)
		(end 30 20)
		(width 0.25)
		(layer "B.Cu")
		(net "A")
		(uuid "00000000-0000-4000-8000-000000000005")
	)
	(segment
		(start 30 30)
		(end 40 30)
		(width 0.3)
		(layer "F.Cu")
		(net "B")
		(locked yes)
		(uuid "00000000-0000-4000-8000-000000000006")
	)
	(segment
		(start 5 40)
		(end 9 40)
		(width 0.2)
		(layer "F.Cu")
		(uuid "00000000-0000-4000-8000-000000000007")
	)
)
"#;

const FRAME: GridFrame = GridFrame { nm_per_unit: 100 };

fn index() -> CopperIndex {
    read(BOARD, None).unwrap().copper_index
}

/// The board's copper as the routed board would have it if nothing changed.
fn unchanged(index: &CopperIndex) -> Vec<FinalItem> {
    index
        .items
        .iter()
        .filter(|i| i.routable)
        .map(|i| match i.kind {
            CopperKind::Via { .. } => {
                let (top, bottom) = i.span.unwrap();
                FinalItem::Via(FinalVia {
                    at: FRAME.to_board(i.start),
                    net: i.net.unwrap(),
                    top,
                    bottom,
                    diameter: i.width.unwrap(),
                    drill: i.drill.unwrap(),
                })
            }
            _ => FinalItem::Trace(FinalTrace {
                layer: i.layer.unwrap(),
                net: i.net.unwrap(),
                half_width: FRAME.half_width(i.width.unwrap()),
                corners: i.points.iter().map(|p| FRAME.to_board(*p)).collect(),
            }),
        })
        .collect()
}

fn trace(net: u32, layer: usize, corners: &[(i64, i64)]) -> FinalItem {
    FinalItem::Trace(FinalTrace {
        layer,
        net,
        half_width: 1250,
        corners: corners.to_vec(),
    })
}

/// mm to board units of the 100 nm frame, Y negated.
fn b(x: f64, y: f64) -> (i64, i64) {
    (
        (x * 10_000.0).round() as i64,
        (-y * 10_000.0).round() as i64,
    )
}

#[test]
fn the_index_keeps_spans_uuids_and_locks() {
    let index = index();
    assert_eq!(index.items.len(), 6);
    for item in &index.items {
        let block = &BOARD[item.span_bytes.0..item.span_bytes.1];
        assert!(block.starts_with('(') && block.ends_with(')'), "{block}");
        let uuid = item.uuid.as_deref().unwrap();
        assert!(block.contains(uuid), "{block}");
    }
    assert!(matches!(index.items[2].kind, CopperKind::Via { .. }));
    assert_eq!(index.items[2].drill, Some(400_000));
    assert!(index.items[4].locked);
    assert!(
        !index.items[5].routable,
        "a track without a net is not on the board"
    );
    assert_eq!(index.style.uuid_token, "uuid");
    assert!(index.style.uuid_quoted);
    assert_eq!(index.uuids.len(), 7);
}

#[test]
fn an_unchanged_board_gives_an_empty_diff() {
    let index = index();
    let d = diff(&index, &unchanged(&index), &FRAME);
    assert!(d.remove.is_empty() && d.add.is_empty(), "{d:?}");
    assert_eq!(d.keep.len(), 6);
}

#[test]
fn a_segment_split_in_two_collinear_parts_is_kept() {
    let index = index();
    let mut items = unchanged(&index);
    // the router split (10,10)-(20,10) at 15 and merged the corner trace
    items[0] = trace(1, 0, &[b(10.0, 10.0), b(15.0, 10.0)]);
    items[1] = trace(1, 0, &[b(15.0, 10.0), b(20.0, 10.0), b(20.0, 20.0)]);
    let d = diff(&index, &items, &FRAME);
    assert!(d.is_empty(), "{d:?}");
}

#[test]
fn a_ripped_and_rerouted_connection_removes_exactly_its_old_blocks() {
    let index = index();
    let mut items = unchanged(&index);
    items[0] = trace(1, 0, &[b(10.0, 10.0), b(10.0, 20.0), b(20.0, 20.0)]);
    items.remove(1);
    let d = diff(&index, &items, &FRAME);
    assert_eq!(d.remove, vec![0, 1]);
    assert_eq!(
        d.add,
        vec![
            NewItem::Segment {
                start: (10_000_000, 10_000_000),
                end: (10_000_000, 20_000_000),
                width: 250_000,
                layer: 0,
                net: 1,
            },
            NewItem::Segment {
                start: (10_000_000, 20_000_000),
                end: (20_000_000, 20_000_000),
                width: 250_000,
                layer: 0,
                net: 1,
            },
        ]
    );
}

#[test]
fn a_locked_original_is_kept_even_when_the_board_lost_it() {
    let index = index();
    let items: Vec<FinalItem> = unchanged(&index)
        .into_iter()
        .filter(|i| !matches!(i, FinalItem::Trace(t) if t.net == 2))
        .collect();
    let d = diff(&index, &items, &FRAME);
    assert!(d.is_empty(), "{d:?}");
}

#[test]
fn a_longer_route_adds_only_the_uncovered_part() {
    let index = index();
    let mut items = unchanged(&index);
    // the B.Cu track now runs on to (35, 20)
    items[3] = trace(1, 1, &[b(20.0, 20.0), b(35.0, 20.0)]);
    let d = diff(&index, &items, &FRAME);
    assert!(d.remove.is_empty());
    assert_eq!(
        d.add,
        vec![NewItem::Segment {
            start: (30_000_000, 20_000_000),
            end: (35_000_000, 20_000_000),
            width: 250_000,
            layer: 1,
            net: 1,
        }]
    );
}

/// A line diff (Myers, O((N+M)D)) as hunks of removed and added lines.
fn line_diff<'a>(a: &[&'a str], b: &[&'a str]) -> (Vec<&'a str>, Vec<&'a str>) {
    let (n, m) = (a.len() as isize, b.len() as isize);
    let max = (n + m) as usize;
    let off = max as isize;
    let mut v = vec![0isize; 2 * max + 2];
    let mut trace: Vec<Vec<isize>> = Vec::new();
    'outer: for d in 0..=max as isize {
        trace.push(v.clone());
        let mut k = -d;
        while k <= d {
            let idx = (k + off) as usize;
            let mut x = if k == -d || (k != d && v[idx - 1] < v[idx + 1]) {
                v[idx + 1]
            } else {
                v[idx - 1] + 1
            };
            let mut y = x - k;
            while x < n && y < m && a[x as usize] == b[y as usize] {
                x += 1;
                y += 1;
            }
            v[idx] = x;
            if x >= n && y >= m {
                break 'outer;
            }
            k += 2;
        }
    }
    let (mut removed, mut added) = (Vec::new(), Vec::new());
    let (mut x, mut y) = (n, m);
    for d in (1..trace.len() as isize).rev() {
        let v = &trace[d as usize];
        let k = x - y;
        let idx = (k + off) as usize;
        let prev_k = if k == -d || (k != d && v[idx - 1] < v[idx + 1]) {
            k + 1
        } else {
            k - 1
        };
        let prev_x = v[(prev_k + off) as usize];
        let prev_y = prev_x - prev_k;
        while x > prev_x && y > prev_y {
            x -= 1;
            y -= 1;
        }
        if x == prev_x {
            added.push(b[prev_y as usize]);
        } else {
            removed.push(a[prev_x as usize]);
        }
        x = prev_x;
        y = prev_y;
    }
    removed.reverse();
    added.reverse();
    (removed, added)
}

#[test]
fn the_text_diff_shows_only_copper_blocks() {
    let index = index();
    let mut items = unchanged(&index);
    items[0] = trace(1, 0, &[b(10.0, 10.0), b(10.0, 20.0), b(20.0, 20.0)]);
    items.remove(1);
    items.push(FinalItem::Via(FinalVia {
        at: b(10.0, 20.0),
        net: 1,
        top: 0,
        bottom: 1,
        diameter: 800_000,
        drill: 400_000,
    }));
    let d = diff(&index, &items, &FRAME);
    let (out, summary) = render(BOARD, &index, &d, fnv1a128(BOARD.as_bytes())).unwrap();
    assert_eq!(
        (summary.removed, summary.added_segments, summary.added_vias),
        (2, 2, 1)
    );
    let (a, bl): (Vec<&str>, Vec<&str>) = (BOARD.lines().collect(), out.lines().collect());
    let (removed, added) = line_diff(&a, &bl);
    // removed: exactly the two old blocks, line for line
    let old: Vec<&str> = index.items[..2]
        .iter()
        .flat_map(|i| BOARD[i.span_bytes.0..i.span_bytes.1].lines())
        .map(|l| l.trim_start())
        .collect();
    let removed_trimmed: Vec<&str> = removed.iter().map(|l| l.trim_start()).collect();
    assert_eq!(removed_trimmed, old);
    // added: whole blocks of the three new items, nothing else
    let added_text = added.join("\n");
    let reparsed = freerouting_kicad::sexpr::Tree::parse(&added_text).unwrap();
    let heads: Vec<&str> = reparsed.root().items().filter_map(|n| n.head()).collect();
    assert_eq!(heads, ["segment", "segment", "via"]);
    // and the output reads back with the new copper
    let back = read(&out, None).unwrap().copper_index;
    assert_eq!(back.items.len(), 7);
    let again = diff(&back, &items, &FRAME);
    assert!(again.is_empty(), "{again:?}");
}

#[test]
fn rendering_is_deterministic_and_survives_a_forced_collision() {
    let index = index();
    let mut items = unchanged(&index);
    items[3] = trace(1, 1, &[b(20.0, 20.0), b(35.0, 20.0)]);
    let d = diff(&index, &items, &FRAME);
    let hash = fnv1a128(BOARD.as_bytes());
    let (one, _) = render(BOARD, &index, &d, hash).unwrap();
    let (two, _) = render(BOARD, &index, &d, hash).unwrap();
    assert_eq!(one, two);
    let fresh = |text: &str| -> Vec<String> {
        let back = read(text, None).unwrap().copper_index;
        back.items
            .iter()
            .filter_map(|i| i.uuid.clone())
            .filter(|u| !index.uuids.contains(u))
            .collect()
    };
    let new_uuid = fresh(&one);
    assert_eq!(new_uuid.len(), 1);
    // the test hook: the would-be UUID is taken, so the next candidate is used
    let gen_with = || {
        let mut g = UuidGen::new(hash, index.uuids.iter().map(String::as_str));
        g.reserve(&new_uuid[0]);
        g
    };
    let (three, _) = render_with(BOARD, &index, &d, gen_with()).unwrap();
    let (four, _) = render_with(BOARD, &index, &d, gen_with()).unwrap();
    assert_eq!(three, four);
    let other = fresh(&three);
    assert_eq!(other.len(), 1);
    assert_ne!(other[0], new_uuid[0]);
}
