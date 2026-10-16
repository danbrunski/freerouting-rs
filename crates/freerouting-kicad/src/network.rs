//! The DSN network (K05.2): nets with their pins, and the net classes.
//!
//! `FromBOARD`, "build the images, components, and netlist" (`specctra_export.cpp`,
//! tag `10.0.0`): the network holds every net that has at least one pin, in
//! net-code order. A net's pins are listed per footprint in file order, per
//! pad in pad order, as `reference-pin`. Pads without a copper layer and
//! copper-less holes are not pins (K05.6).
//!
//! The numeric dialect uses the file's net codes. The name dialect assigns
//! codes by first appearance in the file, with code 0 the empty net, which is
//! what the KiCad 10 export shows for `Issue742`.

use freerouting_dsn::ast::{Circuit, Net, NetClass, PinRef, Rule};

use crate::classes::{ClassTable, ClassValues};
use crate::fmt6g::c_fmt_6g;
use crate::nets::NetTable;
use crate::units::{iu2um, um};

/// `snprintf( "Via[%d-%d]_%.6g:%.6g_um" )`: the padstack name of a via.
pub fn via_name(top: usize, bottom: usize, diameter_nm: i32, drill_nm: i32) -> String {
    format!(
        "Via[{top}-{bottom}]_{}:{}_um",
        c_fmt_6g(um(i64::from(diameter_nm))),
        c_fmt_6g(iu2um(i64::from(drill_nm)))
    )
}

/// The nets with pins, in net-code order. `pins_by_code[code]` lists the
/// pins found for that net, in footprint order.
pub fn build_nets(nets: &NetTable, pins_by_code: &[Vec<PinRef>]) -> Vec<Net> {
    let mut out = Vec::new();
    for code in 1..nets.by_code.len() {
        let Some(pins) = pins_by_code.get(code) else {
            continue;
        };
        if pins.is_empty() {
            continue;
        }
        out.push(Net {
            name: nets.name(code as u32).unwrap_or("").replace('"', "''"),
            subnet: None,
            pins: pins.clone(),
            ordered: false,
            fromtos: Vec::new(),
            rules: Vec::new(),
            circuit: None,
        });
    }
    out
}

/// One class in use: its DSN name, values and nets.
#[derive(Clone, Debug, PartialEq)]
pub struct ClassInUse {
    /// `Default`, a class name or an aggregate (`Power,Default`).
    pub name: String,
    pub values: ClassValues,
    /// Net names, sorted by code point (the order of `std::map<wxString,..>`).
    pub nets: Vec<String>,
}

/// The classes of the board's nets: Default first, then the classes in use
/// in the order of the project file, then aggregates by name.
pub fn classes_in_use(nets: &NetTable, table: &ClassTable) -> Vec<ClassInUse> {
    let mut names: Vec<String> = (1..nets.by_code.len())
        .filter_map(|c| nets.name(c as u32))
        .filter(|n| !n.is_empty())
        .map(|n| n.to_owned())
        .collect();
    names.sort();
    names.dedup();
    let mut by_class: Vec<ClassInUse> = Vec::new();
    for n in &names {
        let eff = table.effective(n);
        match by_class.iter_mut().find(|c| c.name == eff.name) {
            Some(c) => c.nets.push(n.clone()),
            None => by_class.push(ClassInUse {
                name: eff.name,
                values: eff.values,
                nets: vec![n.clone()],
            }),
        }
    }
    let order_of = |name: &str| -> (u8, usize, String) {
        if name == "Default" {
            (0, 0, String::new())
        } else if let Some(i) = table.classes().iter().position(|c| c.name == name) {
            (1, i, String::new())
        } else {
            (2, 0, name.to_owned())
        }
    };
    by_class.sort_by_key(|c| order_of(&c.name));
    if !by_class.iter().any(|c| c.name == "Default") {
        by_class.insert(
            0,
            ClassInUse {
                name: "Default".to_owned(),
                values: table.values_of_name("Default"),
                nets: Vec::new(),
            },
        );
    }
    by_class
}

/// `exportNETCLASS`.
pub fn dsn_class(c: &ClassInUse, copper_count: usize) -> NetClass {
    let mut nc = NetClass::new(if c.name == "Default" {
        "kicad_default".to_owned()
    } else {
        c.name.clone()
    });
    nc.nets = c.nets.clone();
    nc.rules = vec![
        Rule::Width(um(i64::from(c.values.track_width))),
        Rule::Clearance {
            value: um(i64::from(c.values.clearance)),
            types: Vec::new(),
        },
    ];
    nc.circuit = Some(Circuit {
        length: None,
        use_via: vec![via_name(
            0,
            copper_count - 1,
            c.values.via_diameter,
            c.values.via_drill,
        )],
        use_layer: Vec::new(),
    });
    nc
}
