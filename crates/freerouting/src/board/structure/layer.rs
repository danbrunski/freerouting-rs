/// A board layer.
///
/// Port of `board.model.structure.Layer`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Layer {
    pub name: String,
    /// False for a plane (power) layer.
    pub is_signal: bool,
}

impl Layer {
    pub fn new(name: impl Into<String>, is_signal: bool) -> Self {
        Layer {
            name: name.into(),
            is_signal,
        }
    }
}

/// The layers of a board, from top to bottom.
///
/// Port of `board.model.structure.LayerStructure`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LayerStructure {
    pub layers: Vec<Layer>,
}

impl LayerStructure {
    pub fn new(layers: Vec<Layer>) -> Self {
        LayerStructure { layers }
    }

    /// The number of layers.
    pub fn count(&self) -> usize {
        self.layers.len()
    }

    /// The index of the layer named `name` (exact match), or -1.
    pub fn get_no(&self, name: &str) -> i32 {
        self.layers
            .iter()
            .position(|layer| layer.name == name)
            .map_or(-1, |i| i as i32)
    }

    /// The number of signal layers.
    pub fn signal_layer_count(&self) -> usize {
        self.layers.iter().filter(|layer| layer.is_signal).count()
    }

    /// The index of the `no`-th signal layer, or the last layer if there are
    /// fewer signal layers.
    pub fn get_signal_layer(&self, no: usize) -> usize {
        self.layers
            .iter()
            .enumerate()
            .filter(|(_, layer)| layer.is_signal)
            .nth(no)
            .map_or(self.layers.len().saturating_sub(1), |(i, _)| i)
    }

    /// The number of signal layers above layer `layer`, or -1 if `layer` is out
    /// of range.
    pub fn get_signal_layer_no(&self, layer: usize) -> i32 {
        if layer >= self.layers.len() {
            return -1;
        }
        self.layers[..layer].iter().filter(|l| l.is_signal).count() as i32
    }

    /// The layer index of the `signal_layer_no`-th signal layer.
    pub fn get_layer_no(&self, signal_layer_no: usize) -> usize {
        self.get_signal_layer(signal_layer_no)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signal_layer_numbering() {
        let structure = LayerStructure::new(vec![
            Layer::new("Top", true),
            Layer::new("GND", false),
            Layer::new("Bottom", true),
        ]);
        assert_eq!(structure.signal_layer_count(), 2);
        assert_eq!(structure.get_no("GND"), 1);
        assert_eq!(structure.get_no("gnd"), -1);
        assert_eq!(structure.get_signal_layer(1), 2);
        assert_eq!(structure.get_signal_layer(5), 2);
        assert_eq!(structure.get_signal_layer_no(2), 1);
    }
}
