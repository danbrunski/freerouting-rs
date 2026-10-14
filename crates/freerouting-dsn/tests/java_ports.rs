//! Parser-level ports of the Java tests in `src/test/java/app/freerouting/io/specctra/`.
//!
//! The Java tests build a board; these check the syntax tree the board would be
//! built from. Assertions about the board itself (net class merging, plane
//! detection, length violations, ...) belong to the board builder and are not
//! ported here.

use std::path::Path;

use freerouting_dsn::ast::*;
use freerouting_dsn::{Pcb, RulesFile, Session, decode};

fn fixture(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .join(name);
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    decode(&bytes).into_owned()
}

fn round_trip(pcb: &Pcb) {
    let written = pcb.to_dsn();
    assert_eq!(
        &Pcb::parse(&written).unwrap(),
        pcb,
        "round trip of:\n{written}"
    );
}

// ---------------------------------------------------------------------------
// parser/CircuitLengthParserTest
// ---------------------------------------------------------------------------

fn parse_circuit(circuit: &str) -> Circuit {
    let pcb = Pcb::parse(&format!("(pcb t (network (net N {circuit})))")).unwrap();
    round_trip(&pcb);
    pcb.network.unwrap().nets[0].circuit.clone().unwrap()
}

#[test]
fn circuit_length_with_max_and_min() {
    let circuit = parse_circuit("(circuit (length 100 50))");
    assert_eq!(
        circuit.length,
        Some(LengthRule {
            max: 100.0,
            min: Some(50.0)
        })
    );
}

#[test]
fn circuit_length_with_max_only() {
    let circuit = parse_circuit("(circuit (length 100))");
    assert_eq!(
        circuit.length,
        Some(LengthRule {
            max: 100.0,
            min: None
        })
    );
}

#[test]
fn circuit_length_with_min_only_using_negative_sentinel() {
    let circuit = parse_circuit("(circuit (length -1 50))");
    assert_eq!(
        circuit.length,
        Some(LengthRule {
            max: -1.0,
            min: Some(50.0)
        })
    );
}

#[test]
fn circuit_length_with_sub_scope_type_actual() {
    let circuit = parse_circuit("(circuit (length 100.5 45.2 (type actual)))");
    assert_eq!(
        circuit.length,
        Some(LengthRule {
            max: 100.5,
            min: Some(45.2)
        })
    );
}

#[test]
fn circuit_length_max_only_with_sub_scope_type_actual() {
    let circuit = parse_circuit("(circuit (length 100.0 (type actual)))");
    assert_eq!(
        circuit.length,
        Some(LengthRule {
            max: 100.0,
            min: None
        })
    );
}

#[test]
fn circuit_with_use_layer_and_length() {
    let circuit = parse_circuit("(circuit (use_layer F.Cu B.Cu) (length 80 20))");
    assert_eq!(
        circuit.length,
        Some(LengthRule {
            max: 80.0,
            min: Some(20.0)
        })
    );
    assert_eq!(circuit.use_layer, ["F.Cu", "B.Cu"]);
}

// ---------------------------------------------------------------------------
// DsnReaderTest and DsnReaderMetadataTest
// ---------------------------------------------------------------------------

#[test]
fn rpi_splitter_has_two_layers_and_parser_info() {
    let pcb = Pcb::parse(&fixture("Issue143-rpi_splitter.dsn")).unwrap();
    let structure = pcb.structure.as_ref().unwrap();
    assert_eq!(structure.layers.len(), 2);
    let parser = pcb.parser.as_ref().unwrap();
    assert_eq!(parser.host_cad.as_deref(), Some("CadSoft"));
    assert_eq!(parser.string_quote.as_deref(), Some("\""));
    // Metadata: the unit comes from the resolution scope.
    assert_eq!(
        pcb.resolution,
        Some(Resolution {
            unit: Unit::Mil,
            value: 2540
        })
    );
    round_trip(&pcb);
}

#[test]
fn garbage_is_a_parse_error() {
    let err = Pcb::parse("not a dsn file").unwrap_err();
    assert_eq!((err.pos.line, err.pos.column), (1, 1));
    assert!(Pcb::parse("").is_err());
    assert!(Pcb::parse("not dsn").is_err());
}

#[test]
fn design_without_boundary() {
    // The Java test expects anything but success. With the misspelled
    // `stringQuote`, the `"` starts a string that never ends; Java runs into
    // the end of the file, and so does this reader.
    let src = "(pcb test\n  (parser (stringQuote \"))\n  (resolution um 10)\n  (unit um)\n  \
               (structure\n    (layer F.Cu (type signal) (property (index 0)))\n    \
               (layer B.Cu (type signal) (property (index 1)))\n  )\n)\n";
    let err = Pcb::parse(src).unwrap_err();
    assert_eq!((err.pos.line, err.pos.column), (2, 24));
    assert!(err.message.contains("unterminated"), "{err}");

    // Spelled correctly, the file is well formed; the missing outline is the
    // board builder's problem.
    let pcb = Pcb::parse(&src.replace("stringQuote", "string_quote")).unwrap();
    let structure = pcb.structure.as_ref().unwrap();
    assert!(structure.boundaries.is_empty());
    assert_eq!(structure.layers.len(), 2);
    assert_eq!(
        pcb.parser.as_ref().unwrap().string_quote.as_deref(),
        Some("\"")
    );
    assert_eq!(pcb.unit, Some(Unit::Um));
    round_trip(&pcb);
}

#[test]
fn empty_board_has_a_boundary() {
    let pcb = Pcb::parse(&fixture("empty_board.dsn")).unwrap();
    let boundaries = &pcb.structure.as_ref().unwrap().boundaries;
    assert_eq!(boundaries.len(), 1);
    match &boundaries[0].shapes[..] {
        [
            Shape::Path {
                layer: LayerRef::Pcb,
                width,
                points,
            },
        ] => {
            assert_eq!(*width, 0.0);
            assert_eq!(points.len(), 5);
            assert_eq!(points[0], Point::new(186690.0, -107950.0));
            assert_eq!(points[0], points[4]);
        }
        other => panic!("unexpected boundary {other:?}"),
    }
    round_trip(&pcb);
}

#[test]
fn dac2020_bm08_has_kicad_default_class() {
    // Java merges kicad_default into its default class while building the
    // board; the file declares it.
    let pcb = Pcb::parse(&fixture("Issue508-DAC2020_bm08.dsn")).unwrap();
    let network = pcb.network.as_ref().unwrap();
    let class = network
        .classes
        .iter()
        .find(|class| class.name == "kicad_default")
        .unwrap();
    assert!(class.nets.iter().any(|net| net == "/SCL"));
}

#[test]
fn issue034_has_multiple_boundary_paths() {
    let pcb = Pcb::parse(&fixture("Issue034-Green14SegLED.dsn")).unwrap();
    let shapes: Vec<&Shape> = pcb
        .structure
        .as_ref()
        .unwrap()
        .boundaries
        .iter()
        .flat_map(|b| &b.shapes)
        .collect();
    assert!(shapes.len() > 1);
    assert!(shapes.iter().all(|s| matches!(
        s,
        Shape::Path {
            layer: LayerRef::Pcb,
            ..
        }
    )));
    let places: usize = pcb
        .placement
        .as_ref()
        .unwrap()
        .components
        .iter()
        .map(|c| c.places.len())
        .sum();
    assert!(places > 0);
}

fn dsn_with_power_layer(plane: &str) -> String {
    format!(
        "(pcb test\n  (parser (string_quote \") (host_cad \"KiCad's Pcbnew\"))\n  \
         (resolution um 10)\n  (unit um)\n  (structure\n    \
         (layer F.Cu (type signal) (property (index 0)))\n    \
         (layer In1.Cu (type power) (property (index 1)))\n    \
         (layer B.Cu (type signal) (property (index 2)))\n    \
         (boundary (path pcb 0 0 0 0 20000 0 20000 20000 0 20000 0 0))\n{plane}    \
         (via \"Via[0-2]_800:400_um\")\n    (rule (width 200) (clearance 200))\n  )\n  \
         (placement)\n  (library\n    (padstack \"Via[0-2]_800:400_um\"\n      \
         (shape (circle F.Cu 800))\n      (shape (circle In1.Cu 800))\n      \
         (shape (circle B.Cu 800))\n      (attach off)\n    )\n  )\n  (network\n    \
         (net GND)\n    (class kicad_default GND (circuit (use_via \"Via[0-2]_800:400_um\")) \
         (rule (width 200) (clearance 200)))\n  )\n  (wiring)\n)\n"
    )
}

#[test]
fn power_layer_without_plane() {
    let pcb = Pcb::parse(&dsn_with_power_layer("")).unwrap();
    let structure = pcb.structure.as_ref().unwrap();
    assert_eq!(structure.layers[1].layer_type, Some(LayerType::Power));
    assert!(structure.planes.is_empty());
    assert_eq!(
        structure.via.as_ref().unwrap().padstacks,
        ["Via[0-2]_800:400_um"]
    );
    let padstack = &pcb.library.as_ref().unwrap().padstacks[0];
    assert_eq!(padstack.shapes.len(), 3);
    assert_eq!(padstack.attach, Some(false));
    let class = &pcb.network.as_ref().unwrap().classes[0];
    assert_eq!(class.nets, ["GND"]);
    assert_eq!(
        class.circuit.as_ref().unwrap().use_via,
        ["Via[0-2]_800:400_um"]
    );
    assert_eq!(
        class.rules,
        [
            Rule::Width(200.0),
            Rule::Clearance {
                value: 200.0,
                types: vec![]
            }
        ]
    );
    round_trip(&pcb);
}

#[test]
fn power_layer_with_plane() {
    let plane = "    (plane GND (polygon In1.Cu 0 0 0 20000 0 20000 20000 0 20000))\n";
    let pcb = Pcb::parse(&dsn_with_power_layer(plane)).unwrap();
    let planes = &pcb.structure.as_ref().unwrap().planes;
    assert_eq!(planes.len(), 1);
    assert_eq!(planes[0].net, "GND");
    assert_eq!(
        planes[0].area.shape,
        Shape::Polygon {
            layer: LayerRef::Named("In1.Cu".into()),
            aperture_width: 0.0,
            points: vec![
                Point::new(0.0, 0.0),
                Point::new(20000.0, 0.0),
                Point::new(20000.0, 20000.0),
                Point::new(0.0, 20000.0),
            ],
        }
    );
    round_trip(&pcb);
}

#[test]
fn pcbench_box0_power_typed_layers() {
    let pcb = Pcb::parse(&fixture("PCBench-Box0-hv-analog-breakout.dsn")).unwrap();
    let structure = pcb.structure.as_ref().unwrap();
    assert!(
        structure
            .layers
            .iter()
            .any(|layer| layer.layer_type == Some(LayerType::Power))
    );
    assert!(structure.planes.is_empty());
}

#[test]
fn dac2020_bm01_has_host_cad() {
    let pcb = Pcb::parse(&fixture("Issue508-DAC2020_bm01.dsn")).unwrap();
    assert!(pcb.parser.unwrap().host_cad.is_some());
}

#[test]
fn large_design_parses_quickly() {
    let text = fixture("Issue187-processor.Z80.dsn");
    let start = std::time::Instant::now();
    let pcb = Pcb::parse(&text).unwrap();
    round_trip(&pcb);
    assert!(start.elapsed().as_secs() < 5);
}

// ---------------------------------------------------------------------------
// DsnLengthConstraintsTest
// ---------------------------------------------------------------------------

fn length_dsn(network_body: &str) -> String {
    format!(
        "(pcb test_length.dsn\n  (parser\n    (string_quote \")\n    \
         (space_in_quoted_tokens on)\n  )\n  (resolution um 10)\n  (unit um)\n  \
         (structure\n    (layer F.Cu (type signal) (property (index 0)))\n    \
         (boundary\n      (path pcb 0  0 0  20000 0  20000 20000  0 20000  0 0)\n    )\n    \
         (rule (width 250) (clearance 200))\n  )\n  (placement\n    (component PAD\n      \
         (place U1 2000 5000 front 0)\n      (place U2 8000 5000 front 0)\n    )\n  )\n  \
         (library\n    (image PAD\n      (pin CirclePad 1 0 0)\n    )\n    \
         (padstack CirclePad\n      (shape (circle F.Cu 800))\n      (attach off)\n    )\n  )\n  \
         (network\n{network_body}  )\n)\n"
    )
}

fn length(max: f64, min: Option<f64>) -> Option<LengthRule> {
    Some(LengthRule { max, min })
}

#[test]
fn net_class_length_constraint_from_circuit() {
    let pcb = Pcb::parse(&length_dsn(
        "(net N1 (pins U1-1 U2-1))\n(class LENGTH_CLASS N1\n  (circuit (length 5000 1000))\n)\n",
    ))
    .unwrap();
    let network = pcb.network.as_ref().unwrap();
    assert_eq!(network.nets[0].circuit, None);
    assert_eq!(
        network.nets[0].pins,
        [PinRef::new("U1", "1"), PinRef::new("U2", "1")]
    );
    let class = &network.classes[0];
    assert_eq!(class.nets, ["N1"]);
    assert_eq!(
        class.circuit.as_ref().unwrap().length,
        length(5000.0, Some(1000.0))
    );
    round_trip(&pcb);
}

#[test]
fn explicit_net_length_constraint_from_circuit() {
    let pcb = Pcb::parse(&length_dsn(
        "(net N1 (pins U1-1 U2-1)\n  (circuit (length 4000 1500))\n)\n",
    ))
    .unwrap();
    let net = &pcb.network.as_ref().unwrap().nets[0];
    assert_eq!(
        net.circuit.as_ref().unwrap().length,
        length(4000.0, Some(1500.0))
    );
    round_trip(&pcb);
}

#[test]
fn net_and_class_constraints_both_recorded() {
    let pcb = Pcb::parse(&length_dsn(
        "(net N_OVERRIDE (pins U1-1 U2-1)\n  (circuit (length 3500 2500))\n)\n\
         (net N_DEFAULT (pins U1-1 U2-1))\n\
         (class LENGTH_CLASS N_OVERRIDE N_DEFAULT\n  (circuit (length 8000 1000))\n)\n",
    ))
    .unwrap();
    let network = pcb.network.as_ref().unwrap();
    assert_eq!(
        network.nets[0].circuit.as_ref().unwrap().length,
        length(3500.0, Some(2500.0))
    );
    assert_eq!(network.nets[1].circuit, None);
    assert_eq!(network.classes[0].nets, ["N_OVERRIDE", "N_DEFAULT"]);
    assert_eq!(
        network.classes[0].circuit.as_ref().unwrap().length,
        length(8000.0, Some(1000.0))
    );
    round_trip(&pcb);
}

#[test]
fn net_length_constraint_from_rule_scope() {
    let pcb = Pcb::parse(&length_dsn(
        "(net N1 (pins U1-1 U2-1)\n  (rule (length 4500 1200))\n)\n",
    ))
    .unwrap();
    let net = &pcb.network.as_ref().unwrap().nets[0];
    assert_eq!(
        net.rules,
        [Rule::Length(LengthRule {
            max: 4500.0,
            min: Some(1200.0)
        })]
    );
    round_trip(&pcb);
}

#[test]
fn single_value_and_negative_max_constraints() {
    let pcb = Pcb::parse(&length_dsn(
        "(net N_MAX_ONLY (pins U1-1 U2-1)\n  (circuit (length 6000))\n)\n\
         (net N_MIN_ONLY (pins U1-1 U2-1)\n  (circuit (length -1 2200))\n)\n",
    ))
    .unwrap();
    let nets = &pcb.network.as_ref().unwrap().nets;
    assert_eq!(
        nets[0].circuit.as_ref().unwrap().length,
        length(6000.0, None)
    );
    assert_eq!(
        nets[1].circuit.as_ref().unwrap().length,
        length(-1.0, Some(2200.0))
    );
    round_trip(&pcb);
}

// ---------------------------------------------------------------------------
// NonConvexPadstackTest
// ---------------------------------------------------------------------------

#[test]
fn non_convex_polygon_pad() {
    let src = r#"(pcb "test_non_convex_pad.dsn"
  (parser
    (string_quote ")
    (space_in_quoted_tokens on)
    (host_cad "KiCad's Pcbnew")
    (host_version "10.0.0")
  )
  (resolution um 10)
  (unit um)
  (structure
    (layer F.Cu
      (type signal)
      (property (index 0))
    )
    (layer B.Cu
      (type signal)
      (property (index 1))
    )
    (boundary
      (path pcb 0  0 0  10000 0  10000 10000  0 10000  0 0)
    )
    (via "Via[0-1]_800:400_um")
    (rule
      (width 200)
      (clearance 50)
      (clearance 50 (type default_smd))
      (clearance 50 (type smd_smd))
    )
  )
  (placement
    (component "U_TEST:TEST_PKG"
      (place U1 5000 5000 front 0)
    )
  )
  (library
    (image "U_TEST:TEST_PKG"
      (pin Custom_L_Pad 1 0 0)
    )
    (padstack Custom_L_Pad
      (shape (polygon F.Cu 0 -100 -100  100 -100  100 0  0 0  0 100  -100 100))
      (attach off)
    )
    (padstack "Via[0-1]_800:400_um"
      (shape (circle F.Cu 800))
      (shape (circle B.Cu 800))
      (attach off)
    )
  )
  (network
    (net NET1
      (pins U1-1)
    )
  )
)
"#;
    let pcb = Pcb::parse(src).unwrap();
    assert_eq!(pcb.name, "test_non_convex_pad.dsn");
    let library = pcb.library.as_ref().unwrap();
    assert_eq!(library.images[0].name, "U_TEST:TEST_PKG");
    assert_eq!(
        library.images[0].pins[0],
        ImagePin {
            padstack: "Custom_L_Pad".into(),
            name: "1".into(),
            at: Point::new(0.0, 0.0),
            rotation: None,
        }
    );
    match &library.padstacks[0].shapes[..] {
        [
            Shape::Polygon {
                layer,
                aperture_width,
                points,
            },
        ] => {
            assert_eq!(layer, &LayerRef::Named("F.Cu".into()));
            assert_eq!(*aperture_width, 0.0);
            assert_eq!(points.len(), 6);
            assert_eq!(points[2], Point::new(100.0, 0.0));
        }
        other => panic!("unexpected shapes {other:?}"),
    }
    let rules = &pcb.structure.as_ref().unwrap().rules;
    assert_eq!(rules.len(), 4);
    assert_eq!(
        rules[3],
        Rule::Clearance {
            value: 50.0,
            types: vec!["smd_smd".into()]
        }
    );
    let place = &pcb.placement.as_ref().unwrap().components[0].places[0];
    assert_eq!(
        place.location,
        Some(PlaceLocation {
            at: Point::new(5000.0, 5000.0),
            side: Side::Front,
            rotation: 0.0
        })
    );
    round_trip(&pcb);
}

// ---------------------------------------------------------------------------
// RulesRoundTripTest
// ---------------------------------------------------------------------------

#[test]
fn rules_autoroute_settings_round_trip() {
    let settings = AutorouteSettings {
        fanout: Some(false),
        autoroute: Some(true),
        postroute: Some(true),
        vias: Some(true),
        via_costs: Some(75),
        plane_via_costs: Some(8),
        start_ripup_costs: Some(120),
        start_pass_no: None,
        layer_rules: vec![
            AutorouteLayerRule {
                layer: "F.Cu".into(),
                active: Some(true),
                preferred_direction: Some(Direction::Horizontal),
                preferred_direction_trace_costs: Some(1.2),
                against_preferred_direction_trace_costs: Some(2.8),
            },
            AutorouteLayerRule {
                layer: "B.Cu".into(),
                active: Some(true),
                preferred_direction: Some(Direction::Vertical),
                preferred_direction_trace_costs: Some(1.0),
                against_preferred_direction_trace_costs: Some(3.1),
            },
        ],
    };
    let rules = RulesFile {
        design: "Issue029-hw48na".into(),
        items: vec![
            RulesItem::SnapAngle(SnapAngle::FortyfiveDegree),
            RulesItem::AutorouteSettings(settings.clone()),
            RulesItem::Rule(vec![Rule::Width(250.0)]),
        ],
    };
    let text = rules.to_rules();
    assert!(text.starts_with("(rules PCB Issue029-hw48na"));
    assert!(text.contains("(autoroute_settings"));
    assert!(text.contains("(layer_rule"));
    assert!(text.contains("(rule"));
    assert!(text.contains("(snap_angle fortyfive_degree)"));
    let back = RulesFile::parse(&text).unwrap();
    assert_eq!(back, rules);
    match &back.items[1] {
        RulesItem::AutorouteSettings(read) => {
            assert_eq!(read.via_costs, Some(75));
            assert_eq!(read.plane_via_costs, Some(8));
            assert_eq!(read.start_ripup_costs, Some(120));
            assert_eq!(
                read.layer_rules[0].preferred_direction,
                Some(Direction::Horizontal)
            );
            assert_eq!(
                read.layer_rules[1].preferred_direction,
                Some(Direction::Vertical)
            );
            assert_eq!(
                read.layer_rules[0].preferred_direction_trace_costs,
                Some(1.2)
            );
            assert_eq!(
                read.layer_rules[0].against_preferred_direction_trace_costs,
                Some(2.8)
            );
            assert_eq!(
                read.layer_rules[1].preferred_direction_trace_costs,
                Some(1.0)
            );
            assert_eq!(
                read.layer_rules[1].against_preferred_direction_trace_costs,
                Some(3.1)
            );
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn garbage_rules_file_is_an_error() {
    assert!(RulesFile::parse("not a rules file").is_err());
    // The design name is optional, the pcb keyword is not.
    assert!(RulesFile::parse("(rules hw48na)").is_err());
    assert!(RulesFile::parse("(rules PCB)").is_ok());
}

#[test]
fn rules_via_inside_rule_scope_is_skipped() {
    let rules = RulesFile::parse("(rules PCB Issue143\n  (rule\n    (via \"Round1$13\")\n  )\n)\n")
        .unwrap();
    assert_eq!(rules.design, "Issue143");
    assert_eq!(rules.items, [RulesItem::Rule(vec![])]);
}

#[test]
fn existing_rules_fixture() {
    let (rules, warnings) =
        RulesFile::parse_with_warnings(&fixture("Issue029-hw48na_valid.rules")).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(rules.design, "hw48na");
    assert_eq!(
        rules.items[0],
        RulesItem::SnapAngle(SnapAngle::FortyfiveDegree)
    );
    let RulesItem::AutorouteSettings(settings) = &rules.items[1] else {
        panic!("autoroute_settings expected");
    };
    assert_eq!(settings.via_costs, Some(50));
    assert_eq!(settings.start_pass_no, Some(1510));
    assert_eq!(settings.layer_rules.len(), 2);
    assert_eq!(
        settings.layer_rules[0].preferred_direction,
        Some(Direction::Vertical)
    );
    let RulesItem::Rule(default_rules) = &rules.items[2] else {
        panic!("rule expected");
    };
    assert_eq!(default_rules[0], Rule::Width(400.0));
    assert!(default_rules.contains(&Rule::Clearance {
        value: 190.6,
        types: vec!["default".into(), "1A EXTERNAL 1oz".into()],
    }));
    let class = rules
        .items
        .iter()
        .find_map(|item| match item {
            RulesItem::Class(class) if class.name == "1A EXTERNAL 1oz" => Some(class),
            _ => None,
        })
        .unwrap();
    assert!(class.nets.iter().any(|net| net == "GND"));
    assert_eq!(class.clearance_class.as_deref(), Some("1A EXTERNAL 1oz"));
    assert_eq!(class.via_rule.as_deref(), Some("1A EXTERNAL 1oz"));
}

#[test]
fn invalid_rules_fixture_reads_like_java() {
    // `(via_rule 1A EXTERNAL 1oz)` is not quoted: like Java's readStringScope
    // the reader takes the first word and skips the rest with a warning.
    let (rules, warnings) =
        RulesFile::parse_with_warnings(&fixture("Issue029-hw48na_invalid.rules")).unwrap();
    assert!(!warnings.is_empty());
    let class = rules
        .items
        .iter()
        .find_map(|item| match item {
            RulesItem::Class(class) if class.name == "1A EXTERNAL 1oz" => Some(class),
            _ => None,
        })
        .unwrap();
    assert_eq!(class.via_rule.as_deref(), Some("1A"));
}

#[test]
fn degenerate_wire_is_kept() {
    // Java warns about the zero-length wire while inserting it into the
    // board; the tree keeps it as written.
    let pcb = Pcb::parse(&fixture("Issue029-hw48na.dsn")).unwrap();
    let degenerate = pcb
        .wiring
        .as_ref()
        .unwrap()
        .items
        .iter()
        .any(|item| match item {
            WiringItem::Wire(wire) => wire.shapes.iter().any(|shape| match shape {
                Shape::Path { points, .. } => {
                    points.len() >= 2 && points.iter().all(|p| *p == points[0])
                }
                _ => false,
            }),
            WiringItem::Via(_) => false,
        });
    assert!(degenerate);
}

// ---------------------------------------------------------------------------
// SesRoundTripTest
// ---------------------------------------------------------------------------

#[test]
fn ses_mars64_has_wires() {
    let session = Session::parse(&fixture("Issue593-BBD_Mars-64.ses")).unwrap();
    assert_eq!(session.name, "Issue593-BBD_Mars-64.ses");
    assert_eq!(
        session.base_design.as_deref(),
        Some("Issue593-BBD_Mars-64.dsn")
    );
    let routes = session.routes.as_ref().unwrap();
    let wires = routes
        .network_out
        .iter()
        .flat_map(|net| &net.items)
        .filter(|item| matches!(item, WiringItem::Wire(_)))
        .count();
    assert!(wires > 0);
    let placement = session.placement.as_ref().unwrap();
    assert_eq!(
        placement.resolution,
        Some(Resolution {
            unit: Unit::Um,
            value: 10
        })
    );
    assert_eq!(
        placement.components[0].places[0].location.unwrap().side,
        Side::Back
    );
}

#[test]
fn ses_writer_header_and_keywords() {
    let session = Session::parse(&fixture("Issue593-BBD_Mars-64.ses")).unwrap();
    let text = session.to_ses();
    assert!(text.starts_with("(session "));
    assert!(text.contains("(routes"));
    for camel in [
        "(hostCad",
        "(stringQuote",
        "(writeResolution",
        "(viaRule",
        "(pullTight",
        "(shoveFixed",
        "(useLayer",
        "(clearanceClass",
    ] {
        assert!(!text.contains(camel), "{camel}");
    }
    assert_eq!(Session::parse(&text).unwrap(), session);
}

#[test]
fn garbage_session_is_an_error() {
    assert!(Session::parse("garbage").is_err());
    assert!(Session::parse("(pcb x)").is_err());
}

#[test]
fn issue742_session_round_trips() {
    let session = Session::parse(&fixture("Issue742-tastexx-pcb/tastexx-pcb.ses")).unwrap();
    let text = session.to_ses();
    // Placement rotations are written the way KiCad writes them.
    assert!(text.contains(" front 339"));
    assert!(!text.contains("339.0"));
    let mut depth = 0i32;
    for c in text.chars() {
        match c {
            '(' => depth += 1,
            ')' => depth -= 1,
            _ => {}
        }
        assert!(depth >= 0);
    }
    assert_eq!(depth, 0);
    assert_eq!(Session::parse(&text).unwrap(), session);
}
