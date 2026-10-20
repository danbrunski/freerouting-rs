//! `read`: a `.kicad_pcb` to a DSN syntax tree (K05.8).
//!
//! The tree holds the board structure that `FromBOARD` writes before any
//! copper: layers, boundary, default rules, via padstacks, the footprints as
//! images and placements, the padstack library, and the network with its net
//! classes. K06 adds keepouts, planes and wiring.

use std::collections::BTreeSet;

use freerouting_dsn::ast::{
    Area, Boundary, Image, ImagePin, Keepout, KeepoutKind, LayerRef, Library, Net, Network,
    ParserInfo, Pcb, PinRef, Place, PlaceLocation, Placement, Point, Resolution, Rule, Shape, Side,
    Structure, Unit, ViaPadstacks, Wiring, WiringItem,
};

use crate::classes::ClassTable;
use crate::dru::{self, DruFile, DruReport};
use crate::error::{ErrorKind, KicadError, Warning};
use crate::fmt6g::c_fmt_6g;
use crate::footprints::{
    Footprint, Graphic, PadShape, normalize_360, parse_shape, read_footprints,
};
use crate::header;
use crate::image_outlines::image_outlines;
use crate::layers::{Layers, StackupLayer, read_stackup};
use crate::legacy::{self, Angle, rotate_about};
use crate::nets::NetTable;
use crate::network::{build_nets, classes_in_use, dsn_class, via_name};
use crate::outline::board_outline;
use crate::pads::{PadstackSet, make_pads, shape_key};
use crate::project::Project;
use crate::report::ReadReport;
use crate::rule_areas::rule_area_keepouts;
use crate::sexpr::Tree;
use crate::tracks::read_wires;
use crate::units::{map_pt, um};
use crate::vias::read_vias;
use crate::zones::{ZoneLayerMap, cu_stack_order, layer_id_order, make_plane, parse_zone};

/// What the DSN tree cannot carry (X-series length and impedance work).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Extras {
    /// The board stackup, top to bottom, as the file lists it.
    pub stackup: Vec<StackupLayer>,
    /// `(die_length ...)` of pads: component, pin, length in nm.
    pub die_lengths: Vec<DieLength>,
    /// The `.kicad_pro`'s Board Setup minimums (Q18); all unknown without
    /// a project file.
    pub board_minimums: crate::project::BoardMinimums,
}

/// A pad's `die_length`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DieLength {
    pub component: String,
    pub pin: String,
    pub nm: i64,
}

/// The result of [`read`].
#[derive(Clone, Debug, PartialEq)]
pub struct KicadRead {
    pub pcb: Pcb,
    pub warnings: Vec<Warning>,
    pub extras: Extras,
    /// What the reader decided: rule areas, planes, arcs, dropped zones.
    pub report: ReadReport,
    /// What became of each `.kicad_dru` rule ([`crate::dru`], K07.5); empty
    /// (`present == false`) when no rules file was given.
    pub rules: DruReport,
    /// The file's tracks and vias with their byte spans, for the writer
    /// ([`crate::write`], K08.1).
    pub copper_index: crate::write::CopperIndex,
}

/// Options of the reader.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ReadOptions {
    /// Refuse an empty or repeated reference designator with KiCad 10.0.0's
    /// messages ("Footprint with value of '%s' has an empty reference
    /// designator.", "Multiple footprints have the reference designator
    /// '%s'."). KiCad 10.0.7, which made the reference exports of the corpus,
    /// no longer refuses: an empty reference becomes `REF**` and a repeated
    /// one gets `_1`, `_2`, ... (case-insensitively), and the reader does the
    /// same when this is off (the default), with a warning per footprint.
    pub strict_references: bool,
    /// Drop the copper zones that footprints own, as KiCad's DSN export does
    /// (its plane loop walks the board's zones only). Off by default: the
    /// reader keeps them as planes (K00 question 6, K06.4), after the board's
    /// own zones, and warns once per zone.
    pub drop_footprint_zones: bool,
    /// Apply the `.kicad_dru` rules the DSN can express (class-pair
    /// clearances, class track widths). Off by default: KiCad's DSN export
    /// ignores the file, so Java never saw these rules, and applying them
    /// changes routing (K00 question 7). Rules that cannot be expressed are
    /// reported either way ([`KicadRead::rules`]).
    pub apply_design_rules: bool,
}

/// Reads a board with the name `board.dsn` in the tree.
pub fn read(text: &str, project: Option<&str>) -> Result<KicadRead, KicadError> {
    read_named("board.dsn", text, project)
}

/// Reads a board; `name` becomes the `(pcb ...)` name (KiCad uses the board
/// path with `.dsn` in place of `.kicad_pcb`).
pub fn read_named(name: &str, text: &str, project: Option<&str>) -> Result<KicadRead, KicadError> {
    read_with(name, text, project, ReadOptions::default())
}

/// `"` cannot be escaped in a DSN string: KiCad 10.0.7 writes `''`.
fn sanitize(s: &str) -> String {
    s.replace('"', "''")
}

/// [`read_named`] with options.
pub fn read_with(
    name: &str,
    text: &str,
    project: Option<&str>,
    options: ReadOptions,
) -> Result<KicadRead, KicadError> {
    read_with_rules(name, text, project, None, options)
}

/// [`read_with`] and the text of the board's `.kicad_dru` (K07.5). A rules
/// file that does not parse is a warning, not an error: the board is read
/// without it.
pub fn read_with_rules(
    name: &str,
    text: &str,
    project: Option<&str>,
    dru: Option<&str>,
    options: ReadOptions,
) -> Result<KicadRead, KicadError> {
    let trace = std::env::var_os("FR_KICAD_TRACE").is_some();
    let t0 = std::time::Instant::now();
    let lap = |what: &str| {
        if trace {
            eprintln!("read: {what} at {} ms", t0.elapsed().as_millis());
        }
    };
    let mut tree = Tree::parse(text)?;
    lap("parsed");
    let hdr = header::read_header(&tree)?;
    let mut warnings: Vec<Warning> = Vec::new();
    let conv = legacy::normalise(&mut tree, hdr.version);
    warnings.extend(conv.warnings);
    if hdr.is_newer_than_known() {
        warnings.push(Warning {
            line: 0,
            message: format!(
                "board file version {} is newer than the {} this reader was written for",
                hdr.version,
                header::LATEST_KNOWN_VERSION
            ),
        });
    }
    let project = match project {
        Some(t) => {
            let p = Project::parse(t)?;
            for n in &p.notes {
                warnings.push(Warning {
                    line: 0,
                    message: n.clone(),
                });
            }
            p
        }
        None => Project::default(),
    };
    let board = header::board(&tree)?;
    lap("normalised");
    let layers = Layers::read(board, &mut warnings);
    let nets = NetTable::build(&tree, hdr.version);
    lap("nets");
    warnings.extend(nets.warnings.iter().cloned());
    let mut footprints = read_footprints(board, &nets, &mut warnings)?;
    lap("footprints");

    // FromBOARD: component ids must be unique or a session could not come back.
    let mut component_ids: Vec<String> = Vec::with_capacity(footprints.len());
    let mut used: BTreeSet<String> = BTreeSet::new();
    let mut strict_seen: BTreeSet<String> = BTreeSet::new();
    for fp in &footprints {
        if options.strict_references {
            if fp.reference.is_empty() {
                return Err(KicadError {
                    kind: ErrorKind::Semantic,
                    line: tree.node(fp.line).line(),
                    column: 0,
                    message: format!(
                        "Footprint with value of '{}' has an empty reference designator.",
                        fp.value
                    ),
                });
            }
            if !strict_seen.insert(fp.reference.clone()) {
                return Err(KicadError {
                    kind: ErrorKind::Semantic,
                    line: tree.node(fp.line).line(),
                    column: 0,
                    message: format!(
                        "Multiple footprints have the reference designator '{}'.",
                        fp.reference
                    ),
                });
            }
        }
        let base = if fp.reference.is_empty() {
            "REF**".to_owned()
        } else {
            fp.reference.clone()
        };
        let mut unique = base.clone();
        let mut suffix = 1;
        while !used.insert(unique.to_lowercase()) {
            unique = format!("{base}_{suffix}");
            suffix += 1;
        }
        if unique != fp.reference {
            warnings.push(Warning {
                line: tree.node(fp.line).line(),
                message: format!(
                    "footprint reference '{}' is {}; exported as '{unique}' like KiCad 10.0.7",
                    fp.reference,
                    if fp.reference.is_empty() {
                        "empty"
                    } else {
                        "repeated"
                    }
                ),
            });
        }
        component_ids.push(unique);
    }

    // The outline is built before the footprints are flipped.
    let mut board_shapes: Vec<Graphic> = Vec::new();
    for c in board.items().filter(|c| c.is_list()) {
        if matches!(
            c.head(),
            Some("gr_line" | "gr_arc" | "gr_circle" | "gr_rect" | "gr_poly" | "gr_curve")
        ) && let Some(g) = parse_shape(c, None, &mut warnings)?
        {
            board_shapes.push(g);
        }
    }
    let outline = board_outline(&board_shapes, &footprints, project.max_error, true);
    lap("outline");
    if outline.malformed {
        warnings.push(Warning {
            line: 0,
            message: "Board outline is malformed. Run DRC for a full analysis.".to_owned(),
        });
    }

    let copper = layers.count();
    for fp in &mut footprints {
        if fp.back {
            fp.flip_to_front(copper);
        }
    }

    // images, components, netlist
    let mut library_images: Vec<(Image, String)> = Vec::new();
    let mut image_ids: Vec<String> = Vec::new(); // KiCad id of each registered image
    let mut components: Vec<(String, Vec<Place>)> = Vec::new();
    let mut padstack_set = PadstackSet::default();
    let mut report = ReadReport::default();
    let mut pins_by_code: Vec<Vec<PinRef>> = vec![Vec::new(); nets.by_code.len()];
    let mut extras = Extras {
        stackup: read_stackup(board),
        die_lengths: Vec::new(),
        board_minimums: project.minimums,
    };
    for (fp_index, fp) in footprints.iter().enumerate() {
        let component_id = component_ids[fp_index].clone();
        report.custom_pad_hulls += fp
            .pads
            .iter()
            .filter(|p| p.shape == PadShape::Custom)
            .count();
        let pads = make_pads(fp, &layers, project.hole_clearance, project.max_error);
        for d in &pads.padstacks {
            padstack_set.insert(d);
        }
        for pin in &pads.pins {
            if pin.net > 0 {
                pins_by_code[pin.net as usize]
                    .push(PinRef::new(component_id.clone(), pin.id.clone()));
            }
            if let Some(nm) = fp.pads[pin.pad_index].die_length {
                extras.die_lengths.push(DieLength {
                    component: component_id.clone(),
                    pin: pin.id.clone(),
                    nm,
                });
            }
        }
        let outlines = image_outlines(fp);
        let mut keepouts = Vec::new();
        for k in &pads.keepouts {
            for p in 0..copper {
                keepouts.push(Keepout {
                    kind: KeepoutKind::Keepout,
                    area: Area {
                        name: None,
                        shape: Shape::Circle {
                            layer: LayerRef::Named(layers.name(p).to_owned()),
                            diameter: k.diameter,
                            center: k.at,
                        },
                        windows: Vec::new(),
                        clearance_class: None,
                    },
                });
            }
        }
        // rule areas of the footprint: after the copper-less holes, in
        // physical layer order (`LSET::CuStack`), relative to the footprint
        let rel_angle = Angle(-fp.orient).normalize();
        let to_image = |p: legacy::Pt| -> freerouting_dsn::ast::Point {
            let r = rotate_about(p, fp.pos, rel_angle);
            map_pt(
                i64::from(r.0) - i64::from(fp.pos.0),
                i64::from(r.1) - i64::from(fp.pos.1),
            )
        };
        for zone in &fp.zones {
            report.zone_arcs_flattened += zone.arcs as usize;
            if !zone.rule_area {
                let what = zone
                    .name
                    .as_deref()
                    .map(|n| format!(" '{n}'"))
                    .unwrap_or_default();
                if options.drop_footprint_zones {
                    warnings.push(Warning {
                        line: tree.node(zone.node).line(),
                        message: format!(
                            "copper zone{what} of footprint {component_id} dropped, as KiCad's \
                             export drops it"
                        ),
                    });
                    report
                        .footprint_zones_dropped
                        .push((component_id.clone(), zone.name.clone()));
                } else {
                    warnings.push(Warning {
                        line: tree.node(zone.node).line(),
                        message: format!(
                            "copper zone{what} of footprint {component_id} kept as a plane; \
                             KiCad's export drops it"
                        ),
                    });
                    report
                        .footprint_zones_kept
                        .push((component_id.clone(), zone.name.clone()));
                }
                continue;
            }
            let order = cu_stack_order(zone.layers, copper);
            let line = tree.node(zone.node).line();
            let window_layer = layer_id_order(zone.layers, copper).first().copied();
            let (ks, rep) = rule_area_keepouts(
                zone,
                &order,
                &layers,
                Some(&component_id),
                window_layer,
                &to_image,
                &mut warnings,
                line,
            );
            keepouts.extend(ks);
            report.rule_areas.push(rep);
        }
        let image = Image {
            name: fp.id.clone(),
            side: None,
            pins: pads
                .pins
                .iter()
                .map(|p| ImagePin {
                    padstack: p.padstack.clone(),
                    name: p.id.clone(),
                    at: p.at,
                    rotation: if p.rotation != 0.0 {
                        Some(p.rotation)
                    } else {
                        None
                    },
                })
                .collect(),
            outlines,
            keepouts,
        };
        let key = image_key(&image);
        // LookupIMAGE
        let idx = match library_images.iter().position(|(_, k)| *k == key) {
            Some(i) => i,
            None => {
                let dups = image_ids.iter().filter(|id| **id == fp.id).count();
                let mut img = image;
                if dups > 0 {
                    img.name = format!("{}::{}", fp.id, dups);
                }
                image_ids.push(fp.id.clone());
                library_images.push((img, key));
                library_images.len() - 1
            }
        };
        let image_name = library_images[idx].0.name.clone();
        let rel = map_pt(i64::from(fp.pos.0), i64::from(fp.pos.1));
        let rotation = if fp.flipped {
            normalize_360(180.0 - fp.orient)
        } else {
            fp.orient
        };
        let place = Place {
            name: component_id.clone(),
            location: Some(PlaceLocation {
                at: rel,
                side: if fp.flipped { Side::Back } else { Side::Front },
                rotation,
            }),
            position_locked: false,
            pins: Vec::new(),
            keepouts: Vec::new(),
            via_keepouts: Vec::new(),
            place_keepouts: Vec::new(),
            part_number: if fp.value.is_empty() {
                None
            } else {
                Some(sanitize(&fp.value))
            },
        };
        match components.iter_mut().find(|(n, _)| *n == image_name) {
            Some((_, places)) => places.push(place),
            None => components.push((image_name, vec![place])),
        }
    }

    lap("images");
    // classes and vias
    let table = ClassTable::new(&project);
    let in_use = classes_in_use(&nets, &table);
    let default_class = in_use.iter().find(|c| c.name == "Default").cloned();
    let default_values = default_class
        .as_ref()
        .map(|c| c.values)
        .unwrap_or_else(|| table.values_of_name("Default"));
    // (name, diameter, top, bottom, drill)
    let mut via_defs: Vec<(String, i32, usize, usize, i32)> = vec![(
        via_name(
            0,
            copper - 1,
            default_values.via_diameter,
            default_values.via_drill,
        ),
        default_values.via_diameter,
        0,
        copper - 1,
        default_values.via_drill,
    )];
    for c in in_use.iter().filter(|c| c.name != "Default") {
        let n = via_name(0, copper - 1, c.values.via_diameter, c.values.via_drill);
        if !via_defs.iter().any(|(m, ..)| *m == n) {
            via_defs.push((n, c.values.via_diameter, 0, copper - 1, c.values.via_drill));
        }
    }

    // copper: planes, wires and vias (K06.4 to K06.6)
    let zone_map = ZoneLayerMap::from_board(board);
    let mut board_zones = Vec::new();
    for node in board.children("zone") {
        if let Some(z) = parse_zone(node, &zone_map, &nets, false, &mut warnings) {
            report.zone_arcs_flattened += z.arcs as usize;
            board_zones.push(z);
        }
    }
    let (wires, track_stats) = read_wires(board, &layers, &nets, &mut warnings);
    report.track_arcs_flattened = track_stats.arcs_flattened;
    let board_vias = read_vias(board, &layers, &nets, &mut warnings);
    for v in &board_vias {
        if !via_defs.iter().any(|(m, ..)| *m == v.name) {
            via_defs.push((v.name.clone(), v.diameter, v.top, v.bottom, v.drill));
        }
    }
    report.wires = wires.len();
    report.vias = board_vias.len();

    // library padstacks: pads in KiCad's set order, then the vias
    let mut padstacks = padstack_set.into_padstacks();
    for (n, dia, top, bottom, _) in &via_defs {
        padstacks.push(freerouting_dsn::ast::Padstack {
            name: n.clone(),
            shapes: (*top..=*bottom)
                .map(|p| Shape::Circle {
                    layer: LayerRef::Named(layers.name(p).to_owned()),
                    diameter: um(i64::from(*dia)),
                    center: Point::new(0.0, 0.0),
                })
                .collect(),
            attach: Some(false),
            absolute: None,
        });
    }

    // structure
    let mut boundary_shapes = Vec::new();
    let mut keepouts = Vec::new();
    for o in &outline.outlines {
        let close = |pts: &[crate::legacy::Pt]| -> Vec<Point> {
            let mut v: Vec<Point> = pts
                .iter()
                .map(|p| map_pt(i64::from(p.0), i64::from(p.1)))
                .collect();
            if let Some(f) = v.first().copied() {
                v.push(f);
            }
            v
        };
        boundary_shapes.push(Shape::Path {
            layer: LayerRef::Pcb,
            width: 0.0,
            points: close(&o.outer),
        });
        for h in &o.holes {
            keepouts.push(Keepout {
                kind: KeepoutKind::Keepout,
                area: Area {
                    name: None,
                    shape: Shape::Polygon {
                        layer: LayerRef::Signal,
                        aperture_width: 0.0,
                        points: close(h),
                    },
                    windows: Vec::new(),
                    clearance_class: None,
                },
            });
        }
    }
    // rule areas in board zone order, per area in KiCad's layer-id order
    let mut planes = Vec::new();
    let mut no_net_nets: Vec<Net> = Vec::new();
    let board_frame = |p: legacy::Pt| map_pt(i64::from(p.0), i64::from(p.1));
    for zone in board_zones.iter().filter(|z| z.rule_area) {
        let order = layer_id_order(zone.layers, copper);
        let line = tree.node(zone.node).line();
        let (ks, rep) = rule_area_keepouts(
            zone,
            &order,
            &layers,
            None,
            None,
            &board_frame,
            &mut warnings,
            line,
        );
        keepouts.extend(ks);
        report.rule_areas.push(rep);
    }
    // copper zones become planes: zones in board order, layers in layer-id order, then the
    // copper zones that footprints own (in the file's coordinates)
    let footprint_zones = footprints
        .iter()
        .flat_map(|fp| fp.zones_as_filed.iter())
        .filter(|z| !z.rule_area && !options.drop_footprint_zones);
    for zone in board_zones
        .iter()
        .filter(|z| !z.rule_area)
        .chain(footprint_zones)
    {
        for p in layer_id_order(zone.layers, copper) {
            let net_name = match nets
                .name(zone.net)
                .filter(|n| zone.net > 0 && !n.is_empty())
            {
                Some(n) => n.replace('"', "''"),
                None => {
                    // `@:no_net_<n>`, counted per plane; the net goes to the
                    // head of the network
                    let n = format!("@:no_net_{}", no_net_nets.len());
                    no_net_nets.push(Net {
                        name: n.clone(),
                        subnet: None,
                        pins: Vec::new(),
                        ordered: false,
                        fromtos: Vec::new(),
                        rules: Vec::new(),
                        circuit: None,
                    });
                    n
                }
            };
            planes.push(make_plane(zone, layers.name(p), net_name, &board_frame));
        }
    }
    report.planes = planes.len();

    let clearance = um(i64::from(default_values.clearance));
    let structure = Structure {
        layers: layers.dsn_layers(),
        boundaries: vec![Boundary {
            shapes: boundary_shapes,
            clearance_class: None,
        }],
        via: Some(ViaPadstacks {
            padstacks: via_defs.iter().map(|(n, ..)| n.clone()).collect(),
            spare: Vec::new(),
        }),
        rules: vec![
            Rule::Width(um(i64::from(default_values.track_width))),
            Rule::Clearance {
                value: clearance,
                types: Vec::new(),
            },
            Rule::Clearance {
                value: clearance / 4.0,
                types: vec!["smd_smd".to_owned()],
            },
        ],
        keepouts,
        planes,
        ..Structure::default()
    };

    let mut all_nets = no_net_nets;
    all_nets.extend(build_nets(&nets, &pins_by_code));
    let network = Network {
        nets: all_nets,
        classes: in_use.iter().map(|c| dsn_class(c, copper)).collect(),
        ..Network::default()
    };
    let placement = Placement {
        components: components
            .into_iter()
            .map(|(image, places)| freerouting_dsn::ast::ComponentPlacement { image, places })
            .collect(),
        ..Placement::default()
    };
    let mut structure = structure;
    let mut network = network;
    let rules = match dru {
        Some(dru_text) => match DruFile::parse(dru_text) {
            Ok(file) => {
                let copper_names: Vec<String> =
                    (0..copper).map(|p| layers.name(p).to_owned()).collect();
                dru::apply(
                    &file,
                    options.apply_design_rules,
                    &mut network,
                    &mut structure,
                    &copper_names,
                )
            }
            Err(e) => {
                warnings.push(Warning {
                    line: 0,
                    message: format!("the .kicad_dru file could not be read: {e}"),
                });
                DruReport::default()
            }
        },
        None => DruReport::default(),
    };
    let copper_index = crate::write::build_index(
        &tree,
        board,
        &layers,
        &nets,
        via_defs
            .iter()
            .map(
                |(name, diameter, top, bottom, drill)| crate::write::ViaDef {
                    name: name.clone(),
                    diameter: *diameter,
                    drill: *drill,
                    top: *top,
                    bottom: *bottom,
                },
            )
            .collect(),
    );
    let host_version = match &hdr.generator_version {
        Some(v) => format!("{v} (freerouting-kicad)"),
        None => "(freerouting-kicad)".to_owned(),
    };
    let pcb = Pcb {
        name: name.to_owned(),
        parser: Some(ParserInfo {
            string_quote: Some("\"".to_owned()),
            space_in_quoted_tokens: Some(true),
            host_cad: Some("KiCad's Pcbnew".to_owned()),
            host_version: Some(host_version),
            ..ParserInfo::default()
        }),
        resolution: Some(Resolution {
            unit: Unit::Um,
            value: 10,
        }),
        unit: Some(Unit::Um),
        structure: Some(structure),
        placement: Some(placement),
        library: Some(Library {
            images: library_images.into_iter().map(|(i, _)| i).collect(),
            padstacks,
        }),
        part_library: None,
        network: Some(network),
        wiring: Some(Wiring {
            items: wires
                .into_iter()
                .map(WiringItem::Wire)
                .chain(board_vias.iter().map(|v| WiringItem::Via(v.to_dsn())))
                .collect(),
        }),
    };
    Ok(KicadRead {
        pcb,
        warnings,
        extras,
        report,
        rules,
        copper_index,
    })
}

/// `IMAGE::makeHash`: the printed contents without whitespace, parentheses
/// and quotes.
fn image_key(image: &Image) -> String {
    let mut s = String::new();
    for o in &image.outlines {
        s.push_str("outline");
        s.push_str(&shape_key(o));
    }
    for p in &image.pins {
        s.push_str("pin");
        s.push_str(&p.padstack);
        if let Some(r) = p.rotation {
            s.push_str("rotate");
            s.push_str(&c_fmt_6g(r));
        }
        s.push_str(&p.name);
        s.push_str(&c_fmt_6g(p.at.x));
        s.push_str(&c_fmt_6g(p.at.y));
    }
    for k in &image.keepouts {
        s.push_str(k.kind.keyword());
        s.push_str(&shape_key(&k.area.shape));
        for w in &k.area.windows {
            s.push_str("window");
            s.push_str(&shape_key(w));
        }
    }
    s
}

/// For tests and tools: the footprints as the reader sees them after the
/// back-side flip.
pub fn footprints_for_tests(text: &str) -> Result<Vec<Footprint>, KicadError> {
    let tree = Tree::parse(text)?;
    let hdr = header::read_header(&tree)?;
    let board = header::board(&tree)?;
    let layers = Layers::read(board, &mut Vec::new());
    let nets = NetTable::build(&tree, hdr.version);
    let mut fps = read_footprints(board, &nets, &mut Vec::new())?;
    for fp in &mut fps {
        if fp.back {
            fp.flip_to_front(layers.count());
        }
    }
    Ok(fps)
}
