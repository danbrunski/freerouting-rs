//! Grammar details: keywords, aliases, names, quoting, skipped scopes, errors
//! and the writer.

use freerouting_dsn::ast::*;
use freerouting_dsn::{Pcb, RulesFile, Session, decode};

fn parse(src: &str) -> Pcb {
    let pcb = Pcb::parse(src).unwrap_or_else(|e| panic!("{e}"));
    let written = pcb.to_dsn();
    let again = Pcb::parse(&written).unwrap_or_else(|e| panic!("{e} in:\n{written}"));
    assert_eq!(again, pcb, "round trip of:\n{written}");
    pcb
}

const FULL: &str = r#"
# A design that uses most of the grammar.
(PCB "full design"
  (parser
    (string_quote ")
    (space_in_quoted_tokens on)
    (host_cad "KiCad's Pcbnew")
    (host_version 8.0)
    (constant ten 10)
    (write_resolution mil 100)
    (generated_by_freerouting)
  )
  (resolution MM 1000)
  (unit mm)
  (structure
    (layer F.Cu (type signal) (rule (width 0.2)) (property (index 0)))
    (layer In1.Cu (type power) (use_net GND "+3V3"))
    (layer B.Cu (type jumper))
    (layer X.Cu (type mixed))
    (boundary (rectangle pcb 0 0 100 50) (path signal 0 1 1 99 1) (clearance_class edge))
    (via V1 V2 (spare V3))
    (rule (width 0.25) (clear 0.2) (clearance 0.1 (type smd_smd)) (clearance 0.3 (type "a b"-c)))
    (snap_angle ninety_degree)
    (control (via_at_smd on) (off_grid on))
    (flip_style rotate_first)
    (autoroute_settings (vias off) (via_costs 30) (layer_rule F.Cu (prefered_direction horizontal)))
    (keepout "k 1" (circ F.Cu 3 10 10) (clearance_class big))
    (via_keepout (poly signal 0 0 0 1 0 1 1) (window (rect signal 0.2 0.2 0.4 0.4)))
    (place_keepout (rect pcb 0 0 5 5))
    (wire_keepout (rect B.Cu 0 0 1 1))
    (plane GND (polygon In1.Cu 0 0 0 100 0 100 50 0 50))
    (unknown_scope (with (nested "scopes)" here))) # a comment
  )
  (placement
    (place_control (flip_style mirror_first))
    (component "R_0603:R"
      (place R1 10 20 front 90 (lock_type position) (PN 10k))
      (place R2 30.5 -20 back 270.5 (pin 1 (clearance_class tight)) (keepout k1 (clearance_class big)))
      (place R3)
    )
  )
  (library
    (image "R_0603:R"
      (side front)
      (outline (path signal 0.1 -1 -1 1 -1))
      (pin Rect (rotate 90) 1 -0.8 0)
      (pin Rect 2 0.8 0 (rotate 180))
      (pin Rect "with space" 0 1)
      (keepout (rect signal -1 -1 1 1))
    )
    (padstack Rect
      (shape (rect F.Cu -0.4 -0.4 0.4 0.4))
      (shape (circle B.Cu 0.8))
      (shape (polyline_path F.Cu 0.1 0 0 1 1 1 1 2 0))
      (attach off)
      (absolute on)
    )
  )
  (part_library
    (logical_part_mapping LM358 (comp U1 U2))
    (logical_part LM358 (pin 1 0 A 1 OUT 0) (pin 2 0 A 1 IN- 0 (subgate x)))
  )
  (network
    (net GND (pins R1-1 R2-2 "J 1"-"pin 3"))
    (net "+3V3" 2 (order R1-2 R2-1) (rule (width 0.3)) (circuit (use_via V1) (length 50)))
    (net #WLRXD (fromto R1-1 R2-1) (fromto R2-1 R3-1) (layer_rule F.Cu (rule (width 1))))
    (net "")
    (via VIA_A V1 default attach)
    (via VIA_B V2 default)
    (via_rule default VIA_A VIA_B)
    (class power GND "+3V3"
      (clearance_class pc)
      (via_rule default)
      (rule (width 0.5))
      (rule (clearance 0.4))
      (layer_rule F.Cu B.Cu (rule (width 0.6)) (rule (clearance 0.5)))
      (circuit (use_layer F.Cu) (length 10 5))
      (circuit (use_layer B.Cu))
      (shove_fixed on)
      (pull_tight off)
    )
    (class empty)
    (class_class (classes power empty) (rule (clearance 1)))
  )
  (wiring
    (wire (path F.Cu 0.25 0 0 10 0 10 10) (net GND) (clearance_class c1) (type protect))
    (wire (polygon F.Cu 0 0 0 5 0 5 5) (window (circle F.Cu 1 2 2)) (net "+3V3" 2) (type fix))
    (via V1 10 10 (net GND) (type shove_fixed))
    (via V2 -5.5 7 (type route))
    (via V3 0 0 (type "fix"))
  )
)
"#;

#[test]
fn full_design() {
    let pcb = parse(FULL);
    assert_eq!(pcb.name, "full design");

    let parser = pcb.parser.as_ref().unwrap();
    assert_eq!(parser.string_quote.as_deref(), Some("\""));
    assert_eq!(parser.space_in_quoted_tokens, Some(true));
    assert_eq!(parser.host_cad.as_deref(), Some("KiCad's Pcbnew"));
    assert_eq!(parser.host_version.as_deref(), Some("8.0"));
    assert_eq!(parser.constants, [("ten".to_string(), "10".to_string())]);
    assert_eq!(
        parser.write_resolution,
        Some(WriteResolution {
            name: "mil".into(),
            value: 100
        })
    );
    assert!(parser.generated_by_freerouting);
    assert_eq!(
        pcb.resolution,
        Some(Resolution {
            unit: Unit::Mm,
            value: 1000
        })
    );
    assert_eq!(pcb.unit, Some(Unit::Mm));

    let s = pcb.structure.as_ref().unwrap();
    let types: Vec<_> = s.layers.iter().map(|l| l.layer_type.clone()).collect();
    assert_eq!(
        types,
        [
            Some(LayerType::Signal),
            Some(LayerType::Power),
            Some(LayerType::Jumper),
            Some(LayerType::Other("mixed".into()))
        ]
    );
    assert_eq!(s.layers[0].rules, [Rule::Width(0.2)]);
    assert_eq!(s.layers[1].use_net, ["GND", "+3V3"]);
    assert_eq!(
        s.boundaries[0].shapes[0],
        Shape::Rect {
            layer: LayerRef::Pcb,
            x1: 0.0,
            y1: 0.0,
            x2: 100.0,
            y2: 50.0
        }
    );
    assert_eq!(s.boundaries[0].shapes[1].layer(), &LayerRef::Signal);
    assert_eq!(s.boundaries[0].clearance_class.as_deref(), Some("edge"));
    assert_eq!(
        s.via,
        Some(ViaPadstacks {
            padstacks: vec!["V1".into(), "V2".into()],
            spare: vec!["V3".into()]
        })
    );
    assert_eq!(
        s.rules,
        [
            Rule::Width(0.25),
            Rule::Clearance {
                value: 0.2,
                types: vec![]
            },
            Rule::Clearance {
                value: 0.1,
                types: vec!["smd_smd".into()]
            },
            Rule::Clearance {
                value: 0.3,
                types: vec!["a b".into(), "c".into()]
            },
        ]
    );
    assert_eq!(s.snap_angle, Some(SnapAngle::NinetyDegree));
    assert_eq!(
        s.control,
        Some(Control {
            via_at_smd: Some(true)
        })
    );
    assert_eq!(s.flip_style_rotate_first, Some(true));
    let settings = s.autoroute_settings.as_ref().unwrap();
    assert_eq!(settings.vias, Some(false));
    assert_eq!(settings.via_costs, Some(30));
    assert_eq!(
        settings.layer_rules[0].preferred_direction,
        Some(Direction::Horizontal)
    );
    let kinds: Vec<_> = s.keepouts.iter().map(|k| k.kind).collect();
    assert_eq!(
        kinds,
        [
            KeepoutKind::Keepout,
            KeepoutKind::ViaKeepout,
            KeepoutKind::PlaceKeepout,
            KeepoutKind::WireKeepout
        ]
    );
    assert_eq!(s.keepouts[0].area.name.as_deref(), Some("k 1"));
    assert_eq!(
        s.keepouts[0].area.shape,
        Shape::Circle {
            layer: LayerRef::Named("F.Cu".into()),
            diameter: 3.0,
            center: Point::new(10.0, 10.0)
        }
    );
    assert_eq!(s.keepouts[0].area.clearance_class.as_deref(), Some("big"));
    assert_eq!(s.keepouts[1].area.windows.len(), 1);
    assert_eq!(s.planes[0].net, "GND");

    let placement = pcb.placement.as_ref().unwrap();
    assert_eq!(placement.flip_style_rotate_first, Some(false));
    let places = &placement.components[0].places;
    assert_eq!(placement.components[0].image, "R_0603:R");
    assert!(places[0].position_locked);
    assert_eq!(places[0].part_number.as_deref(), Some("10k"));
    assert_eq!(
        places[1].location,
        Some(PlaceLocation {
            at: Point::new(30.5, -20.0),
            side: Side::Back,
            rotation: 270.5
        })
    );
    assert_eq!(
        places[1].pins,
        [ItemClearance {
            name: "1".into(),
            clearance_class: "tight".into()
        }]
    );
    assert_eq!(places[1].keepouts[0].name, "k1");
    assert_eq!(places[2].location, None);

    let library = pcb.library.as_ref().unwrap();
    let image = &library.images[0];
    assert_eq!(image.side, Some(Side::Front));
    assert_eq!(image.outlines.len(), 1);
    assert_eq!(image.pins[0].rotation, Some(90.0));
    assert_eq!(image.pins[0].name, "1");
    assert_eq!(image.pins[1].rotation, Some(180.0));
    assert_eq!(image.pins[2].name, "with space");
    assert_eq!(image.keepouts.len(), 1);
    let padstack = &library.padstacks[0];
    assert_eq!(padstack.shapes.len(), 3);
    assert_eq!(
        padstack.shapes[2],
        Shape::PolylinePath {
            layer: LayerRef::Named("F.Cu".into()),
            width: 0.1,
            points: vec![
                Point::new(0.0, 0.0),
                Point::new(1.0, 1.0),
                Point::new(1.0, 1.0),
                Point::new(2.0, 0.0)
            ]
        }
    );
    assert_eq!(
        (padstack.attach, padstack.absolute),
        (Some(false), Some(true))
    );

    let parts = pcb.part_library.as_ref().unwrap();
    assert_eq!(parts.mappings[0].components, ["U1", "U2"]);
    assert_eq!(parts.parts[0].pins[1].gate_pin, "IN-");

    let network = pcb.network.as_ref().unwrap();
    assert_eq!(
        network.nets[0].pins,
        [
            PinRef::new("R1", "1"),
            PinRef::new("R2", "2"),
            PinRef::new("J 1", "pin 3")
        ]
    );
    let net = &network.nets[1];
    assert_eq!(
        (net.name.as_str(), net.subnet, net.ordered),
        ("+3V3", Some(2), true)
    );
    assert_eq!(net.rules, [Rule::Width(0.3)]);
    assert_eq!(net.circuit.as_ref().unwrap().use_via, ["V1"]);
    assert_eq!(network.nets[2].name, "#WLRXD");
    assert_eq!(network.nets[2].fromtos.len(), 2);
    assert_eq!(network.nets[3].name, "");
    assert!(network.vias[0].attach);
    assert!(!network.vias[1].attach);
    assert_eq!(
        network.via_rules[0],
        ViaRule {
            name: "default".into(),
            vias: vec!["VIA_A".into(), "VIA_B".into()]
        }
    );
    let class = &network.classes[0];
    assert_eq!(class.nets, ["GND", "+3V3"]);
    assert_eq!(class.clearance_class.as_deref(), Some("pc"));
    assert_eq!(class.via_rule.as_deref(), Some("default"));
    assert_eq!(class.rules.len(), 2);
    assert_eq!(class.layer_rules[0].layers, ["F.Cu", "B.Cu"]);
    assert_eq!(class.layer_rules[0].rules.len(), 2);
    // Circuits merge like Java: the length of the last one (none), the layers
    // of all of them.
    let circuit = class.circuit.as_ref().unwrap();
    assert_eq!(circuit.length, None);
    assert_eq!(circuit.use_layer, ["F.Cu", "B.Cu"]);
    assert_eq!(
        (class.shove_fixed, class.pull_tight),
        (Some(true), Some(false))
    );
    assert!(network.classes[1].nets.is_empty());
    assert_eq!(network.class_classes[0].classes, ["power", "empty"]);

    let items = &pcb.wiring.as_ref().unwrap().items;
    let WiringItem::Wire(wire) = &items[0] else {
        panic!()
    };
    assert_eq!(
        wire.net,
        Some(NetId {
            name: "GND".into(),
            subnet: None
        })
    );
    assert_eq!(wire.clearance_class.as_deref(), Some("c1"));
    assert_eq!(wire.wire_type, Some(WireType::Protect));
    let WiringItem::Wire(area) = &items[1] else {
        panic!()
    };
    assert!(matches!(area.shapes[0], Shape::Polygon { .. }));
    assert_eq!(area.windows.len(), 1);
    assert_eq!(area.net.as_ref().unwrap().subnet, Some(2));
    assert_eq!(area.wire_type, Some(WireType::Fix));
    let types: Vec<_> = items[2..]
        .iter()
        .map(|item| match item {
            WiringItem::Via(via) => via.via_type.clone(),
            WiringItem::Wire(_) => None,
        })
        .collect();
    assert_eq!(
        types,
        [
            Some(WireType::ShoveFixed),
            Some(WireType::Other("route".into())),
            Some(WireType::Other("fix".into()))
        ]
    );
    assert_eq!(
        WireType::Other("route".into()).fixed_state(),
        FixedState::UserFixed
    );
}

/// K06.1: `wire_keepout` is its own kind in the structure and image scopes and is written
/// back as `wire_keepout`; in a place scope (names only) it reads as `keepout`.
#[test]
fn wire_keepout_keeps_its_kind() {
    let pcb = parse(
        "(pcb x (structure (WIRE_KEEPOUT \"a\" (rect B.Cu 0 0 1 1)) (keepout (rect B.Cu 0 0 2 2)) \
         (wire_keepout (rect F.Cu 0 0 3 3))) \
         (placement (component img (place U1 1 2 front 0 (wire_keepout k (clearance_class big))))) \
         (library (image img (wire_keepout (rect F.Cu 0 0 1 1)) (keepout (rect F.Cu 0 0 2 2)))))",
    );
    let s = pcb.structure.as_ref().unwrap();
    let kinds: Vec<_> = s.keepouts.iter().map(|k| k.kind).collect();
    assert_eq!(
        kinds,
        [
            KeepoutKind::WireKeepout,
            KeepoutKind::Keepout,
            KeepoutKind::WireKeepout
        ]
    );
    assert_eq!(s.keepouts[0].area.name.as_deref(), Some("a"));
    let image = &pcb.library.as_ref().unwrap().images[0];
    assert_eq!(image.keepouts[0].kind, KeepoutKind::WireKeepout);
    assert_eq!(image.keepouts[1].kind, KeepoutKind::Keepout);
    let place = &pcb.placement.as_ref().unwrap().components[0].places[0];
    assert_eq!(place.keepouts.len(), 1);
    assert_eq!(place.keepouts[0].name, "k");
    let text = pcb.to_dsn();
    assert_eq!(text.matches("(wire_keepout").count(), 3, "{text}");
    assert_eq!(KeepoutKind::WireKeepout.keyword(), "wire_keepout");
}

#[test]
fn keywords_ignore_case_and_aliases() {
    let pcb = parse(
        "(Pcb x (STRUCTURE (Boundary (RECT PCB 0 0 1 1)) (Rule (CLEAR 3) (Width 2))) \
         (wiring (WIRE (PATH Signal 1 0 0 1 1))))",
    );
    let structure = pcb.structure.unwrap();
    assert_eq!(structure.boundaries[0].shapes[0].layer(), &LayerRef::Pcb);
    assert_eq!(
        structure.rules,
        [
            Rule::Clearance {
                value: 3.0,
                types: vec![]
            },
            Rule::Width(2.0)
        ]
    );
    let WiringItem::Wire(wire) = &pcb.wiring.unwrap().items[0] else {
        panic!()
    };
    assert_eq!(wire.shapes[0].layer(), &LayerRef::Signal);
}

#[test]
fn quoted_keyword_is_not_a_keyword() {
    let (pcb, warnings) = Pcb::parse_with_warnings("(pcb x (\"structure\" (layer A)))").unwrap();
    assert_eq!(pcb.structure, None);
    assert_eq!(warnings.len(), 1);
    assert_eq!((warnings[0].pos.line, warnings[0].pos.column), (1, 8));
}

#[test]
fn unknown_scopes_and_stray_words_are_skipped() {
    let src = "(pcb x\n  (foo (bar \"(\" #baz) ())\n  stray\n  (structure (layer A (property (index 0))) (()) (layer B))\n)";
    let (pcb, warnings) = Pcb::parse_with_warnings(src).unwrap();
    let names: Vec<_> = pcb
        .structure
        .unwrap()
        .layers
        .into_iter()
        .map(|l| l.name)
        .collect();
    assert_eq!(names, ["A", "B"]);
    let positions: Vec<_> = warnings
        .iter()
        .map(|w| (w.pos.line, w.pos.column))
        .collect();
    // Q09.6: the named unknown scope `foo` (pcb) warns too, besides the stray
    // word and the scope without keyword; KiCad's `property` inside `layer`
    // is on the quiet list and does not.
    assert_eq!(positions, [(2, 4), (3, 3), (4, 45)]);
    assert!(warnings[0].message.contains("'foo' in pcb"), "{warnings:?}");
    assert!(
        warnings.iter().all(|w| !w.message.contains("property")),
        "{warnings:?}"
    );
}

/// Q09.6: the quiet list is keyed by the enclosing scope as well as the
/// keyword. `property` inside `layer` (KiCad's stack-up index, no
/// constraint) is skipped silently; the same keyword anywhere else warns.
#[test]
fn only_listed_kicad_scopes_skip_without_a_warning() {
    let quiet = "(pcb x (structure (layer A (property (index 0))) (layer B (property (index 1)))))";
    let (_, warnings) = Pcb::parse_with_warnings(quiet).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");

    let loud = "(pcb x (structure (property (index 0)) (layer A (property (index 0)))))";
    let (_, warnings) = Pcb::parse_with_warnings(loud).unwrap();
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert!(
        warnings[0].message.contains("'property' in structure"),
        "{warnings:?}"
    );
}

/// Q09.6: an invented `(foo_keepout ...)` in the structure scope gets one
/// warning naming `foo_keepout` and `structure`, at its first position; a
/// second one in the same file is not reported again. What is read does not
/// change.
#[test]
fn an_unknown_keepout_warns_once_naming_keyword_and_scope() {
    let src = "(pcb x\n  (structure\n    (layer A)\n    (foo_keepout (rect A 0 0 1 1))\n    \
               (keepout (rect A 0 0 2 2))\n    (foo_keepout (rect A 0 0 3 3))))";
    let (pcb, warnings) = Pcb::parse_with_warnings(src).unwrap();
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    let warning = &warnings[0];
    assert!(warning.message.contains("'foo_keepout'"), "{warning}");
    assert!(warning.message.contains("in structure"), "{warning}");
    assert_eq!((warning.pos.line, warning.pos.column), (4, 6));
    let structure = pcb.structure.unwrap();
    assert_eq!(structure.keepouts.len(), 1);
    assert_eq!(structure.keepouts[0].kind, KeepoutKind::Keepout);
    // The same text without the invented scopes reads the same tree.
    let plain = Pcb::parse("(pcb x\n  (structure\n    (layer A)\n    (keepout (rect A 0 0 2 2))))")
        .unwrap();
    assert_eq!(plain.structure.unwrap().keepouts, structure.keepouts);
}

fn fixture_text(name: &str) -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .join(name);
    decode(&std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))).into_owned()
}

/// Q09.1: `Issue754-avionics_hub.dsn` round-trips with its `wire_keepout`
/// intact: read as `WireKeepout`, written back as `wire_keepout`, and read
/// again to the same tree.
#[test]
fn issue754_round_trips_its_wire_keepout() {
    let pcb = Pcb::parse(&fixture_text("Issue754-avionics_hub.dsn")).unwrap();
    let kinds: Vec<_> = pcb
        .structure
        .as_ref()
        .unwrap()
        .keepouts
        .iter()
        .map(|k| k.kind)
        .collect();
    assert_eq!(kinds, [KeepoutKind::WireKeepout]);
    let written = pcb.to_dsn();
    assert_eq!(written.matches("(wire_keepout").count(), 1);
    assert_eq!(written.matches("(keepout").count(), 0);
    let again = Pcb::parse(&written).unwrap();
    assert_eq!(again, pcb);
}

/// Q09.6's tally: the unknown scope keywords of every DSN, SES and rules file
/// under `fixtures/`, with the number of files each is reported in. Run with
/// `--ignored --nocapture`; the result is in the Q09.6 done note.
#[test]
#[ignore = "prints the tally of unknown scopes across the fixtures"]
fn tally_unknown_scopes_across_fixtures() {
    fn collect(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        let mut entries: Vec<_> = std::fs::read_dir(dir).unwrap().flatten().collect();
        entries.sort_by_key(|e| e.path());
        for entry in entries {
            let path = entry.path();
            if path.is_dir() {
                if path.file_name().is_some_and(|n| n == "reference") {
                    continue;
                }
                collect(&path, out);
            } else {
                out.push(path);
            }
        }
    }
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures");
    let mut files = Vec::new();
    collect(&root, &mut files);
    let mut tally: std::collections::BTreeMap<String, usize> = Default::default();
    let mut scanned = 0;
    for path in files {
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        let text = decode(&bytes);
        let warnings = match ext {
            "dsn" => Pcb::parse_with_warnings(&text).map(|(_, w)| w),
            "ses" => Session::parse_with_warnings(&text).map(|(_, w)| w),
            "rules" => RulesFile::parse_with_warnings(&text).map(|(_, w)| w),
            _ => continue,
        };
        let Ok(warnings) = warnings else {
            continue;
        };
        scanned += 1;
        for w in warnings {
            if let Some(rest) = w.message.strip_prefix("unknown scope '") {
                let (keyword, rest) = rest.split_once('\'').unwrap();
                let scope = rest
                    .trim_start_matches(" in ")
                    .split(' ')
                    .next()
                    .unwrap_or("");
                *tally.entry(format!("{ext} {scope}/{keyword}")).or_default() += 1;
            }
        }
    }
    println!("{scanned} files scanned");
    for (key, count) in &tally {
        println!("{count:5}  {key}");
    }
}

#[test]
fn comments_are_skipped() {
    let pcb =
        parse("# header\n(pcb x /* block\n comment */ (resolution mil 10) # trailing\n /**/)");
    assert_eq!(pcb.resolution.unwrap().value, 10);
}

#[test]
fn missing_final_bracket_is_a_warning() {
    let (pcb, warnings) = Pcb::parse_with_warnings("(pcb x (resolution um 10)\n").unwrap();
    assert!(pcb.resolution.is_some());
    assert_eq!(warnings.len(), 1);
}

#[test]
fn errors_carry_positions() {
    let cases = [
        (
            "(pcb x\n  (wiring (via V 1 abc)))",
            (2, 20),
            "number expected",
        ),
        ("(pcb x\n  (structure (layer A)", (2, 23), "end of file"),
        (
            "(pcb x (resolution furlong 10))",
            (1, 20),
            "resolution unit",
        ),
        ("(pcb x (resolution mil 2.5))", (1, 24), "integer expected"),
        ("(session x (routes (network_out (net", (1, 37), "net name"),
        ("(rules x)", (1, 8), "pcb"),
        ("(foo)", (1, 2), "not a Specctra design file"),
    ];
    for (src, (line, column), text) in cases {
        let err = if src.starts_with("(session") {
            Session::parse(src).unwrap_err()
        } else if src.starts_with("(rules") {
            RulesFile::parse(src).unwrap_err()
        } else {
            Pcb::parse(src).unwrap_err()
        };
        assert_eq!(
            (err.pos.line, err.pos.column),
            (line, column),
            "{src}: {err}"
        );
        assert!(err.message.contains(text), "{src}: {err}");
        assert!(
            err.to_string()
                .starts_with(&format!("line {line}, column {column}: "))
        );
    }
}

#[test]
fn invalid_shapes_are_skipped_with_a_warning() {
    let (pcb, warnings) = Pcb::parse_with_warnings(
        "(pcb x (structure (boundary (rect pcb 0 0 1) (path pcb 0 0 0 x 1) (polyline pcb 1) \
         (polygon pcb 0 0 0 1 1 2))))",
    )
    .unwrap();
    let shapes = &pcb.structure.unwrap().boundaries[0].shapes;
    // Only the polygon survives, without its dangling coordinate.
    assert_eq!(
        shapes[..],
        [Shape::Polygon {
            layer: LayerRef::Pcb,
            aperture_width: 0.0,
            points: vec![Point::new(0.0, 0.0), Point::new(1.0, 1.0)]
        }][..]
    );
    // The short rect, the bad path and the odd polygon; `polyline` is an
    // unknown scope, skipped as in Java, with a warning since Q09.6.
    assert_eq!(warnings.len(), 4);
    assert_eq!(
        warnings
            .iter()
            .filter(|w| w.message.contains("unknown scope 'polyline' in boundary"))
            .count(),
        1,
        "{warnings:?}"
    );
}

#[test]
fn pin_references() {
    let pcb = parse(
        "(pcb x (network (net N (pins \"J3 -+\"-\"_VBUS #22\" J2-VBUS u1-7e9 U-1-5 'Q 1'-2 R1-\"\" \
         \"J2(--)\"-GND \u{416}1-\u{3a9}))))",
    );
    let pins = &pcb.network.unwrap().nets[0].pins;
    assert_eq!(
        pins[..],
        [
            PinRef::new("J3 -+", "_VBUS #22"),
            PinRef::new("J2", "VBUS"),
            PinRef::new("u1", "7e9"),
            PinRef::new("U", "1-5"),
            PinRef::new("Q 1", "2"),
            PinRef::new("R1", ""),
            PinRef::new("J2(--)", "GND"),
            PinRef::new("\u{416}1", "\u{3a9}"),
        ]
    );
}

#[test]
fn clearance_types_split_like_java() {
    let pcb = parse(
        "(pcb x (structure (rule (clearance 1 (type default_\"1A EXTERNAL 1oz\")) \
         (clearance 2 (type \"A\"-\"B\")) (clearance 3 (type \"kicad_default\"_\"1A X\")) \
         (clearance 4 (type \"\" smd_smd)))))",
    );
    let types: Vec<Vec<String>> = pcb
        .structure
        .unwrap()
        .rules
        .into_iter()
        .map(|rule| match rule {
            Rule::Clearance { types, .. } => types,
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(
        types,
        [
            vec!["default_\"1A", "EXTERNAL", "1oz\""],
            vec!["A", "B"],
            vec!["kicad_default", "_\"1A", "X\""],
            vec!["smd_smd"],
        ]
    );
}

#[test]
fn class_lists_skip_empty_names_and_span_lines() {
    let pcb = parse(
        "(pcb x (network (class kicad_default \"\" GND\n  \"Net-(D1-Pad2)\" 'VCC' /SDA\n  \
         (rule (width 1)))))",
    );
    let class = &pcb.network.unwrap().classes[0];
    assert_eq!(class.name, "kicad_default");
    assert_eq!(class.nets, ["GND", "Net-(D1-Pad2)", "VCC", "/SDA"]);
}

#[test]
fn class_with_names_only() {
    // Java loses track of the brackets here; this reader does not.
    let pcb = parse("(pcb x (network (class A n1 n2) (class B n3)))");
    let classes = &pcb.network.unwrap().classes;
    assert_eq!(classes[0].nets, ["n1", "n2"]);
    assert_eq!(classes[1].nets, ["n3"]);
}

#[test]
fn non_ascii_names_are_kept() {
    let pcb = parse(
        "(pcb \u{41f}\u{43b}\u{430}\u{442}\u{430} (network (net \u{3bc}V (pins R\u{3a9}-1)) \
         (class \u{4e2d}\u{6587} \u{3bc}V)))",
    );
    assert_eq!(pcb.name, "\u{41f}\u{43b}\u{430}\u{442}\u{430}");
    let network = pcb.network.unwrap();
    assert_eq!(network.nets[0].name, "\u{3bc}V");
    assert_eq!(network.nets[0].pins[0].component, "R\u{3a9}");
    assert_eq!(network.classes[0].name, "\u{4e2d}\u{6587}");
}

#[test]
fn place_without_side_or_rotation() {
    let (pcb, warnings) = Pcb::parse_with_warnings(
        "(pcb x (placement (component C (place R1 1 2) (place R2 3 4 back))))",
    )
    .unwrap();
    let places = &pcb.placement.unwrap().components[0].places;
    assert_eq!(places[0].location.unwrap().side, Side::Front);
    assert_eq!(places[1].location.unwrap().side, Side::Back);
    assert_eq!(places[1].location.unwrap().rotation, 0.0);
    assert_eq!(warnings.len(), 3);
}

#[test]
fn component_and_place_control_at_pcb_level() {
    let pcb = parse(
        "(pcb x (component C (place R1 1 2 front 0)) (place_control (flip_style rotate_first)))",
    );
    let placement = pcb.placement.unwrap();
    assert_eq!(placement.components[0].places[0].name, "R1");
    assert_eq!(placement.flip_style_rotate_first, Some(true));
}

#[test]
fn repeated_scopes_merge() {
    let pcb = parse(
        "(pcb x (structure (layer A) (via V1)) (structure (layer B) (via V2)) \
         (network (net N1)) (network (net N2)))",
    );
    let structure = pcb.structure.unwrap();
    assert_eq!(structure.layers.len(), 2);
    assert_eq!(structure.via.unwrap().padstacks, ["V2"]);
    assert_eq!(pcb.network.unwrap().nets.len(), 2);
}

#[test]
fn writer_quotes_names_that_need_it() {
    let names = [
        "plain",
        "",
        "with space",
        "tab\there",
        "(paren",
        "close)",
        "\"starts with quote",
        "'single",
        "has \"double\" inside",
        "both \" and '",
        "#hash",
        "/*comment",
        "mid\"quote",
        "-dash",
        "pcb",
        "signal",
        "123",
        "line\nbreak",
        "\u{416}",
    ];
    for name in names {
        if name == "both \" and '" {
            continue; // not representable in Specctra syntax
        }
        let pcb = Pcb {
            name: name.to_string(),
            structure: Some(Structure {
                layers: vec![Layer {
                    name: name.to_string(),
                    layer_type: Some(LayerType::Other(name.to_string())),
                    rules: vec![Rule::Clearance {
                        value: 1.0,
                        types: if name.is_empty() {
                            vec![]
                        } else {
                            vec![name.to_string(), "x".into()]
                        },
                    }],
                    use_net: vec![name.to_string()],
                }],
                ..Structure::default()
            }),
            network: Some(Network {
                nets: vec![Net {
                    name: name.to_string(),
                    subnet: None,
                    pins: vec![PinRef::new(name, name)],
                    ordered: false,
                    fromtos: vec![],
                    rules: vec![],
                    circuit: None,
                }],
                classes: vec![NetClass {
                    nets: if name.is_empty() {
                        vec![]
                    } else {
                        vec![name.to_string()]
                    },
                    ..NetClass::new(name)
                }],
                ..Network::default()
            }),
            wiring: Some(Wiring {
                items: vec![WiringItem::Via(Via {
                    padstack: name.to_string(),
                    at: Point::new(0.1, -1e-7),
                    net: Some(NetId {
                        name: name.to_string(),
                        subnet: Some(-3),
                    }),
                    clearance_class: Some(name.to_string()),
                    via_type: Some(WireType::Other(name.to_string())),
                })],
            }),
            ..Pcb::default()
        };
        let text = pcb.to_dsn();
        let back = Pcb::parse(&text).unwrap_or_else(|e| panic!("{name:?}: {e}\n{text}"));
        assert_eq!(back, pcb, "{name:?}:\n{text}");
    }
}

#[test]
fn layer_names_pcb_and_signal() {
    // Quoted or not, any case: pcb and signal are the special layers.
    let pcb =
        parse("(pcb x (structure (boundary (path \"pcb\" 0 0 0 1 1) (rect SIGNAL 0 0 1 1))))");
    let shapes = &pcb.structure.unwrap().boundaries[0].shapes;
    assert_eq!(shapes[0].layer(), &LayerRef::Pcb);
    assert_eq!(shapes[1].layer(), &LayerRef::Signal);
    assert_eq!(LayerRef::from_name("F.Cu"), LayerRef::Named("F.Cu".into()));
}

#[test]
fn numbers_round_trip_exactly() {
    let values = [
        0.0,
        -0.5,
        0.1,
        1.0 / 3.0,
        1e-9,
        123456789.123,
        1e20,
        2.5e-3,
        162598.1,
    ];
    let pcb = Pcb {
        name: "n".into(),
        wiring: Some(Wiring {
            items: vec![WiringItem::Wire(Wire {
                shapes: vec![Shape::Path {
                    layer: LayerRef::Signal,
                    width: 0.25,
                    points: values.iter().map(|&v| Point::new(v, -v)).collect(),
                }],
                windows: vec![],
                net: None,
                clearance_class: None,
                wire_type: None,
                attr: None,
            })],
        }),
        ..Pcb::default()
    };
    let text = pcb.to_dsn();
    let numbers = text
        .split(|c: char| c.is_whitespace() || c == '(' || c == ')')
        .filter(|word| word.starts_with(|c: char| c.is_ascii_digit() || c == '-'));
    for number in numbers {
        assert!(!number.contains('e'), "{number} in {text}");
    }
    assert_eq!(Pcb::parse(&text).unwrap(), pcb);
    let pcb = parse("(pcb x (wiring (via V .5 -.25e1) (via W +3 007)))");
    let ats: Vec<Point> = pcb
        .wiring
        .unwrap()
        .items
        .iter()
        .map(|item| match item {
            WiringItem::Via(via) => via.at,
            WiringItem::Wire(_) => panic!(),
        })
        .collect();
    assert_eq!(ats, [Point::new(0.5, -2.5), Point::new(3.0, 7.0)]);
}

#[test]
fn writer_output_format() {
    let pcb = Pcb::parse(
        "(pcb demo (resolution um 10) (structure (layer F.Cu (type signal)) \
         (boundary (path pcb 0 0 0 10 0 10 10 0 10 0 0))) (network (net GND (pins R1-1 R2-1))))",
    )
    .unwrap();
    let expected = "\
(pcb demo
  (resolution um 10)
  (structure
    (layer F.Cu
      (type signal)
    )
    (boundary
      (path pcb 0
        0 0 10 0 10 10 0 10
        0 0
      )
    )
  )
  (network
    (net GND
      (pins R1-1 R2-1)
    )
  )
)
";
    assert_eq!(pcb.to_dsn(), expected);
}

#[test]
fn session_round_trip_with_pin_swaps() {
    let src = "(session s1 (base_design \"d 1.dsn\")\n\
               (placement (resolution mil 1000) (component C (place R1 1 2 front 0) (place R2 3 4 back 180.5)))\n\
               (was_is (pins R1-1 R1-2) (pins U1-3 U1-4 U1-5))\n\
               (routes (resolution mil 1000) (parser (host_cad KiCad) (host_version 7))\n\
               (library_out (padstack via0 (shape (circle F.Cu 600)) (shape (circle B.Cu 600)) (attach off)))\n\
               (network_out (net GND (wire (path F.Cu 250 0 0 100 0) (type protect)) (via via0 100 0)) (net \"+5V\"))))";
    let (session, warnings) = Session::parse_with_warnings(src).unwrap();
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert_eq!(session.name, "s1");
    assert_eq!(session.base_design.as_deref(), Some("d 1.dsn"));
    assert_eq!(
        session.was_is.as_ref().unwrap()[..],
        [
            PinSwap {
                was: PinRef::new("R1", "1"),
                is: PinRef::new("R1", "2")
            },
            PinSwap {
                was: PinRef::new("U1", "3"),
                is: PinRef::new("U1", "4")
            },
        ]
    );
    let routes = session.routes.as_ref().unwrap();
    assert_eq!(
        routes.parser.as_ref().unwrap().host_version.as_deref(),
        Some("7")
    );
    assert_eq!(routes.library_out[0].shapes.len(), 2);
    assert_eq!(routes.network_out[0].items.len(), 2);
    assert_eq!(routes.network_out[1].name, "+5V");
    let text = session.to_ses();
    assert_eq!(Session::parse(&text).unwrap(), session);
}

#[test]
fn session_component_without_image_name() {
    // Java's SES writer leaves the image name out for unnamed packages.
    let src = "(session s (placement (component (place @HOLE0 1 2 front 0)) \
               (component P (place R1 3 4 back 0))))";
    let (session, warnings) = Session::parse_with_warnings(src).unwrap();
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    let placement = session.placement.as_ref().unwrap();
    assert_eq!(placement.components[0].image, "");
    assert_eq!(placement.components[0].places[0].name, "@HOLE0");
    assert_eq!(placement.components[1].image, "P");
    let text = session.to_ses();
    assert_eq!(Session::parse(&text).unwrap(), session);
}

#[test]
fn rules_file_round_trip() {
    let src = "(rules PCB board\n (snap_angle fortyfive_degree)\n (rule (width 250) (clear 200))\n \
               (layer F.Cu (rule (width 300)))\n (padstack v (shape (circle F.Cu 600)))\n \
               (via v v default)\n (via_rule default v)\n (class default (clearance_class default)))";
    let rules = RulesFile::parse(src).unwrap();
    assert_eq!(rules.items.len(), 7);
    assert_eq!(
        rules.items[2],
        RulesItem::Layer {
            name: "F.Cu".into(),
            rules: vec![Rule::Width(300.0)]
        }
    );
    assert_eq!(RulesFile::parse(&rules.to_rules()).unwrap(), rules);
}

#[test]
fn decode_falls_back_to_latin1() {
    assert_eq!(decode(b"(pcb x)"), "(pcb x)");
    let latin1 = b"(pcb \xb5V)";
    let text = decode(latin1);
    assert_eq!(text, "(pcb \u{b5}V)");
    assert_eq!(Pcb::parse(&text).unwrap().name, "\u{b5}V");
}
