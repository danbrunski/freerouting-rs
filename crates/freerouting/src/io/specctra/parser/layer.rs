//! Layers as the DSN reader sees them.

/// A layer of the DSN file, or one of the pseudo layers `pcb` and `signal`.
///
/// Port of `io.specctra.parser.Layer`.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct DsnLayer {
    pub name: String,
    /// The layer number, -1 for `pcb` and `signal`.
    pub no: i32,
    pub is_signal: bool,
    /// The nets of a power layer (`use_net`).
    pub net_names: Vec<String>,
}

impl DsnLayer {
    pub fn new(name: impl Into<String>, no: i32, is_signal: bool, net_names: Vec<String>) -> Self {
        DsnLayer {
            name: name.into(),
            no,
            is_signal,
            net_names,
        }
    }

    /// The pseudo layer `pcb`.
    pub fn pcb() -> Self {
        DsnLayer::new("pcb", -1, false, Vec::new())
    }

    /// The pseudo layer `signal`: all signal layers.
    pub fn signal() -> Self {
        DsnLayer::new("signal", -1, true, Vec::new())
    }

    pub fn is_pcb(&self) -> bool {
        self.no < 0 && !self.is_signal && self.name == "pcb"
    }

    pub fn is_signal_pseudo_layer(&self) -> bool {
        self.no < 0 && self.is_signal && self.name == "signal"
    }
}

/// The layers of the DSN file.
///
/// Port of `io.specctra.parser.LayerStructure`.
#[derive(Clone, Debug, Default)]
pub(crate) struct DsnLayerStructure {
    pub layers: Vec<DsnLayer>,
}

impl DsnLayerStructure {
    pub fn new(layers: Vec<DsnLayer>) -> Self {
        DsnLayerStructure { layers }
    }

    /// The index of the layer named `name` (exact match). Names containing
    /// `Top` or `Bottom` (Electra's outline layers) map to the first or the
    /// last layer. -1 if not found.
    pub fn get_no(&self, name: &str) -> i32 {
        if let Some(index) = self.layers.iter().position(|l| l.name == name) {
            return index as i32;
        }
        if name.contains("Top") {
            return 0;
        }
        if name.contains("Bottom") {
            return self.layers.len() as i32 - 1;
        }
        -1
    }

    /// True if a power layer uses the net `net_name`.
    pub fn contains_plane(&self, net_name: &str) -> bool {
        self.layers
            .iter()
            .any(|l| !l.is_signal && l.net_names.iter().any(|n| n == net_name))
    }
}
