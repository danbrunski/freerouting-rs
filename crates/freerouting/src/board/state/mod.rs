//! Board state shared with the outside: host information, units and the
//! changed area of the last edit.
//!
//! Port of `board.state.Communication` and `board.state.ChangedArea`. The
//! observer interfaces and the display coordinate transform belong to the
//! GUI and are not ported.

use freerouting_geometry::{FloatPoint, IntBox, IntOctagon, PolylineShapeOps, TileShape};

use crate::board::structure::Unit;
use crate::datastructures::ItemIdGenerator;
use crate::io::CoordinateTransform;

/// The `write_resolution` of a DSN parser scope.
///
/// Port of `Communication.SpecctraParserInfo.WriteResolution`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WriteResolution {
    pub char_name: String,
    pub positive_int: i32,
}

/// What the DSN file said about the program that wrote it.
///
/// Port of `Communication.SpecctraParserInfo`.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct SpecctraParserInfo {
    pub string_quote: String,
    pub host_cad: Option<String>,
    pub host_version: Option<String>,
    pub constants: Vec<(String, String)>,
    pub write_resolution: Option<WriteResolution>,
    pub dsn_file_generated_by_host: bool,
}

/// Units, resolution, host information and the id generator of a board.
///
/// Port of `board.state.Communication` without the observers.
#[derive(Clone, Debug)]
pub struct Communication {
    pub coordinate_transform: CoordinateTransform,
    pub unit: Unit,
    /// Board units per `unit`.
    pub resolution: i32,
    pub specctra_parser_info: Option<SpecctraParserInfo>,
    pub id_generator: ItemIdGenerator,
}

impl Default for Communication {
    fn default() -> Self {
        Communication {
            coordinate_transform: CoordinateTransform::new(1.0, 0.0, 0.0),
            unit: Unit::Mil,
            resolution: 1,
            specctra_parser_info: Some(SpecctraParserInfo {
                string_quote: "\"".into(),
                ..SpecctraParserInfo::default()
            }),
            id_generator: ItemIdGenerator::new(),
        }
    }
}

impl Communication {
    fn host_cad_lower(&self) -> Option<String> {
        self.specctra_parser_info
            .as_ref()?
            .host_cad
            .as_ref()
            .map(|s| s.to_lowercase())
    }

    /// True if the file was written by Fusion 360 or Eagle.
    pub fn host_cad_is_fusion(&self) -> bool {
        self.host_cad_lower().is_some_and(|cad| {
            cad.contains("fusion") || cad.contains("cadsoft") || cad.contains("eagle")
        })
    }

    pub fn host_cad_is_kicad(&self) -> bool {
        self.host_cad_lower().is_some_and(|cad| cad.contains("kicad"))
    }

    /// True if the file was written by KiCad 5 or older: the first number in
    /// the host version is at most 5.
    pub fn host_is_old_kicad(&self) -> bool {
        let Some(info) = &self.specctra_parser_info else {
            return false;
        };
        let (Some(cad), Some(version)) = (&info.host_cad, &info.host_version) else {
            return false;
        };
        if !cad.to_lowercase().contains("kicad") {
            return false;
        }
        let digits: String = version
            .chars()
            .skip_while(|c| !c.is_ascii_digit())
            .take_while(char::is_ascii_digit)
            .collect();
        match digits.parse::<i64>() {
            Ok(number) => number <= 5,
            // Java's Integer.parseInt throws on overflow.
            Err(_) => false,
        }
    }

    pub fn host_cad_exists(&self) -> bool {
        self.specctra_parser_info
            .as_ref()
            .is_some_and(|info| info.host_cad.is_some())
    }

    /// The board resolution expressed per `unit`.
    pub fn get_resolution(&self, unit: Unit) -> f64 {
        Unit::scale(f64::from(self.resolution), unit, self.unit)
    }
}

#[derive(Clone, Copy, Debug)]
struct MutableOctagon {
    lx: f64,
    ly: f64,
    rx: f64,
    uy: f64,
    ulx: f64,
    lrx: f64,
    llx: f64,
    urx: f64,
}

impl MutableOctagon {
    const EMPTY: MutableOctagon = MutableOctagon {
        lx: i32::MAX as f64,
        ly: i32::MAX as f64,
        rx: i32::MIN as f64,
        uy: i32::MIN as f64,
        ulx: i32::MAX as f64,
        lrx: i32::MIN as f64,
        llx: i32::MAX as f64,
        urx: i32::MIN as f64,
    };

    fn to_int(self) -> IntOctagon {
        if self.rx < self.lx || self.uy < self.ly || self.lrx < self.ulx || self.urx < self.llx {
            return IntOctagon::EMPTY;
        }
        IntOctagon::new(
            self.lx.floor() as i32,
            self.ly.floor() as i32,
            self.rx.ceil() as i32,
            self.uy.ceil() as i32,
            self.ulx.floor() as i32,
            self.lrx.ceil() as i32,
            self.llx.floor() as i32,
            self.urx.ceil() as i32,
        )
    }
}

/// The region changed by an edit, as an octagon per layer.
///
/// Port of `board.state.ChangedArea`.
#[derive(Clone, Debug)]
pub struct ChangedArea {
    arr: Vec<MutableOctagon>,
}

impl ChangedArea {
    pub fn new(layer_count: usize) -> Self {
        ChangedArea {
            arr: vec![MutableOctagon::EMPTY; layer_count],
        }
    }

    /// Enlarges the area on `layer` to contain `point`.
    pub fn join(&mut self, point: FloatPoint, layer: usize) {
        let c = &mut self.arr[layer];
        c.lx = point.x.min(c.lx);
        c.ly = point.y.min(c.ly);
        c.rx = c.rx.max(point.x);
        c.uy = c.uy.max(point.y);
        let tmp = point.x - point.y;
        c.ulx = c.ulx.min(tmp);
        c.lrx = c.lrx.max(tmp);
        let tmp = point.x + point.y;
        c.llx = c.llx.min(tmp);
        c.urx = c.urx.max(tmp);
    }

    /// Enlarges the area on `layer` to contain the corners of `shape`.
    pub fn join_shape(&mut self, shape: &TileShape, layer: usize) {
        for i in 0..shape.border_line_count() {
            self.join(shape.corner_approx(i), layer);
        }
    }

    /// The changed area on `layer`.
    pub fn get_area(&self, layer: usize) -> IntOctagon {
        self.arr[layer].to_int()
    }

    /// The box around the changed areas of all layers.
    pub fn surrounding_box(&self) -> IntBox {
        let (mut llx, mut lly, mut urx, mut ury) = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
        for c in &self.arr {
            llx = llx.min(c.lx.floor() as i32);
            lly = lly.min(c.ly.floor() as i32);
            urx = urx.max(c.rx.ceil() as i32);
            ury = ury.max(c.uy.ceil() as i32);
        }
        if llx > urx || lly > ury {
            return IntBox::EMPTY;
        }
        IntBox::new(llx, lly, urx, ury)
    }

    pub fn set_empty(&mut self, layer: usize) {
        self.arr[layer] = MutableOctagon::EMPTY;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_kicad_detection() {
        let mut communication = Communication::default();
        let info = communication.specctra_parser_info.as_mut().unwrap();
        info.host_cad = Some("KiCad's Pcbnew".into());
        info.host_version = Some("(5.1.5)-3".into());
        assert!(communication.host_is_old_kicad());
        communication.specctra_parser_info.as_mut().unwrap().host_version =
            Some("7.0.1".into());
        assert!(!communication.host_is_old_kicad());
        assert!(communication.host_cad_is_kicad());
        assert!(communication.host_cad_exists());
    }

    #[test]
    fn changed_area_grows_with_points() {
        let mut area = ChangedArea::new(2);
        assert!(area.get_area(0).is_empty());
        area.join(FloatPoint::new(1.5, 2.0), 0);
        area.join(FloatPoint::new(-3.0, 4.2), 0);
        assert_eq!(area.surrounding_box(), IntBox::new(-3, 2, 2, 5));
        area.set_empty(0);
        assert_eq!(area.surrounding_box(), IntBox::EMPTY);
    }
}
