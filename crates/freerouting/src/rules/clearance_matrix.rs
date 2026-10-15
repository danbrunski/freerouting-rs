use crate::util::equals_ignore_case;

/// The value added to clearances when a safety margin is requested.
pub const CLEARANCE_SAFETY_MARGIN: i32 = 16;

#[derive(Clone, Debug)]
struct Row {
    name: String,
    /// `column[i][layer]`: the clearance from class `i` to this row's class.
    column: Vec<Vec<i32>>,
    max_value: Vec<i32>,
}

/// The required clearances between the clearance classes of items, per layer.
///
/// Port of `rules.ClearanceMatrix`. Class 0 (`"null"`) is the class of items
/// without clearance; class 1 is the default class.
#[derive(Clone, Debug)]
pub struct ClearanceMatrix {
    layer_count: usize,
    max_value_on_layer: Vec<i32>,
    rows: Vec<Row>,
}

impl ClearanceMatrix {
    /// A matrix with `names.len()` classes (at least 1) and all values 0.
    pub fn new(layer_count: usize, names: &[&str]) -> Self {
        let class_count = names.len().max(1);
        let rows = (0..class_count)
            .map(|i| Row {
                name: names.get(i).copied().unwrap_or("null").to_string(),
                column: vec![vec![0; layer_count]; class_count],
                max_value: vec![0; layer_count],
            })
            .collect();
        ClearanceMatrix {
            layer_count,
            max_value_on_layer: vec![0; layer_count],
            rows,
        }
    }

    /// The matrix with the classes `"null"` and `"default"`, all clearances
    /// between non-null classes set to `default_value`.
    pub fn default_instance(layer_count: usize, default_value: i32) -> Self {
        let mut result = ClearanceMatrix::new(layer_count, &["null", "default"]);
        result.set_default_value(default_value);
        result
    }

    /// The index of the class named `name` (any case), or -1.
    pub fn get_no(&self, name: &str) -> i32 {
        self.rows
            .iter()
            .position(|row| equals_ignore_case(&row.name, name))
            .map_or(-1, |i| i as i32)
    }

    /// The name of class `index`.
    pub fn get_name(&self, index: i32) -> Option<&str> {
        if index < 0 {
            return None;
        }
        self.rows.get(index as usize).map(|row| row.name.as_str())
    }

    /// Sets the clearance between all non-null classes on all layers.
    pub fn set_default_value(&mut self, value: i32) {
        for layer in 0..self.layer_count {
            self.set_default_value_on_layer(layer, value);
        }
    }

    /// Sets the clearance between all non-null classes on `layer`.
    pub fn set_default_value_on_layer(&mut self, layer: usize, value: i32) {
        let n = self.rows.len();
        for i in 1..n {
            for j in 1..n {
                self.set_value_on_layer(i, j, layer, value);
            }
        }
    }

    /// Sets the clearance from class `i` to class `j` on all layers.
    pub fn set_value(&mut self, i: usize, j: usize, value: i32) {
        for layer in 0..self.layer_count {
            self.set_value_on_layer(i, j, layer, value);
        }
    }

    /// Sets the clearance from class `i` to class `j` on `layer`. Negative
    /// values become 0 and odd values are rounded up to the next even value.
    pub fn set_value_on_layer(&mut self, i: usize, j: usize, layer: usize, value: i32) {
        let mut value = value.max(0);
        if value % 2 != 0 {
            if value == i32::MAX {
                value -= 1;
            } else {
                value += 1;
            }
        }
        let row = &mut self.rows[j];
        row.column[i][layer] = value;
        row.max_value[layer] = row.max_value[layer].max(value);
        self.max_value_on_layer[layer] = self.max_value_on_layer[layer].max(value);
    }

    /// Sets the clearance from class `i` to class `j` on the inner layers.
    pub fn set_inner_value(&mut self, i: usize, j: usize, value: i32) {
        for layer in 1..self.layer_count.saturating_sub(1) {
            self.set_value_on_layer(i, j, layer, value);
        }
    }

    /// The clearance from class `i` to class `j` on `layer`, plus
    /// [`CLEARANCE_SAFETY_MARGIN`] if `add_safety_margin`. 0 if an argument is
    /// out of range.
    pub fn get_value(&self, i: i32, j: i32, layer: i32, add_safety_margin: bool) -> i32 {
        let n = self.rows.len() as i32;
        if i < 0 || i >= n || j < 0 || j >= n || layer < 0 || layer >= self.layer_count as i32 {
            return 0;
        }
        let value = self.rows[j as usize].column[i as usize][layer as usize];
        if add_safety_margin {
            value + CLEARANCE_SAFETY_MARGIN
        } else {
            value
        }
    }

    /// The largest clearance of class `i` on `layer` (both clamped into
    /// range).
    pub fn max_value(&self, i: i32, layer: i32) -> i32 {
        let i = i.max(0).min(self.rows.len() as i32 - 1) as usize;
        let layer = layer.max(0).min(self.layer_count as i32 - 1);
        if layer < 0 {
            return 0;
        }
        self.rows[i].max_value[layer as usize]
    }

    /// The largest clearance on `layer` (clamped into range).
    pub fn max_value_on_layer(&self, layer: i32) -> i32 {
        let layer = layer.max(0).min(self.layer_count as i32 - 1);
        if layer < 0 {
            return 0;
        }
        self.max_value_on_layer[layer as usize]
    }

    /// True if the clearance between `i` and `j` differs between layers.
    pub fn is_layer_dependent(&self, i: usize, j: usize) -> bool {
        let values = &self.rows[j].column[i];
        values.iter().any(|v| *v != values[0])
    }

    /// True if the clearance between `i` and `j` differs between inner
    /// layers.
    pub fn is_inner_layer_dependent(&self, i: usize, j: usize) -> bool {
        if self.layer_count <= 2 {
            return false;
        }
        let values = &self.rows[j].column[i];
        values[2..self.layer_count - 1]
            .iter()
            .any(|v| *v != values[1])
    }

    /// The number of classes.
    pub fn class_count(&self) -> usize {
        self.rows.len()
    }

    /// The number of layers.
    pub fn layer_count(&self) -> usize {
        self.layer_count
    }

    /// Half the clearance of class `index` to itself, rounded up: the amount
    /// by which shapes of that class are enlarged in a compensated search
    /// tree.
    pub fn clearance_compensation_value(&self, index: i32, layer: i32) -> i32 {
        (self.get_value(index, index, layer, false) + 1) / 2
    }

    /// Appends a class named `name` with the clearances of the default class.
    /// Returns false if a class with that name exists.
    pub fn append_class(&mut self, name: &str) -> bool {
        if self.get_no(name) >= 0 {
            return false;
        }
        let old_count = self.rows.len();
        for row in &mut self.rows {
            row.column.push(vec![0; self.layer_count]);
        }
        self.rows.push(Row {
            name: name.to_string(),
            column: vec![vec![0; self.layer_count]; old_count + 1],
            max_value: vec![0; self.layer_count],
        });
        for i in 0..old_count {
            for layer in 0..self.layer_count {
                let default_value = self.get_value(1, i as i32, layer as i32, false);
                self.set_value_on_layer(old_count, i, layer, default_value);
                self.set_value_on_layer(i, old_count, layer, default_value);
            }
        }
        for layer in 0..self.layer_count {
            let default_value = self.get_value(1, 1, layer as i32, false);
            self.set_value_on_layer(old_count, old_count, layer, default_value);
        }
        true
    }

    /// Removes class `index`. As in Java, the per-row maximum values of the
    /// remaining rows are reset to 0.
    pub fn remove_class(&mut self, index: usize) {
        self.rows.remove(index);
        for row in &mut self.rows {
            row.column.remove(index);
            row.max_value = vec![0; self.layer_count];
        }
    }

    /// True if classes `first` and `second` have the same clearances to all
    /// non-null classes.
    pub fn is_equal(&self, first: i32, second: i32) -> bool {
        if first == second {
            return true;
        }
        let n = self.rows.len() as i32;
        if first < 0 || second < 0 || first >= n || second >= n {
            return false;
        }
        let (r1, r2) = (&self.rows[first as usize], &self.rows[second as usize]);
        (1..self.rows.len()).all(|i| r1.column[i] == r2.column[i])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_are_even_and_not_negative() {
        let mut matrix = ClearanceMatrix::default_instance(2, 0);
        matrix.set_value_on_layer(1, 1, 0, 7);
        assert_eq!(matrix.get_value(1, 1, 0, false), 8);
        matrix.set_value_on_layer(1, 1, 1, -5);
        assert_eq!(matrix.get_value(1, 1, 1, false), 0);
        assert_eq!(matrix.get_value(1, 1, 0, true), 8 + CLEARANCE_SAFETY_MARGIN);
        assert_eq!(matrix.get_value(5, 1, 0, false), 0);
        assert_eq!(matrix.clearance_compensation_value(1, 0), 4);
    }

    #[test]
    fn appended_class_copies_the_default_class() {
        let mut matrix = ClearanceMatrix::default_instance(1, 200);
        assert!(matrix.append_class("power"));
        assert!(!matrix.append_class("POWER"));
        let power = matrix.get_no("Power");
        assert_eq!(power, 2);
        assert_eq!(matrix.get_value(power, 1, 0, false), 200);
        assert_eq!(matrix.get_value(power, power, 0, false), 200);
        assert_eq!(matrix.get_value(0, power, 0, false), 0);
        assert_eq!(matrix.get_name(2), Some("power"));
        assert_eq!(matrix.max_value(2, 0), 200);
    }
}
