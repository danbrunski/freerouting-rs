//! Lower bounds for the routing of a board.
//!
//! Port of `core.scoring.BoardStatisticsBoundsCalculator` and
//! `BoardStatisticsBounds`.

use std::collections::{BTreeMap, BTreeSet};

use crate::board::BasicBoard;
use crate::board::items::ItemKind;
use crate::board::structure::Unit;

/// Lower bounds for trace length, vias and bends.
///
/// Port of `core.scoring.BoardStatisticsBounds`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BoardStatisticsBounds {
    /// The sum of the per-net minimum spanning trees (Manhattan distance) in
    /// millimeters.
    pub min_trace_length_mm: f32,
    /// The vias needed to connect the layers of the terminals of each net.
    pub min_via_count: i32,
    /// The MST edges that are neither horizontal nor vertical.
    pub min_bend_count: i32,
}

struct Terminal {
    x: f64,
    y: f64,
    signal_layers: BTreeSet<i32>,
}

/// The connected groups of layers (union-find).
#[derive(Default)]
struct LayerGroups {
    parent: BTreeMap<i32, i32>,
}

impl LayerGroups {
    fn add_layers(&mut self, layers: &BTreeSet<i32>) {
        let mut first = None;
        for &layer in layers {
            self.parent.entry(layer).or_insert(layer);
            match first {
                None => first = Some(layer),
                Some(first) => self.union(first, layer),
            }
        }
    }

    fn find(&mut self, layer: i32) -> i32 {
        let parent = self.parent[&layer];
        if parent == layer {
            return layer;
        }
        let root = self.find(parent);
        self.parent.insert(layer, root);
        root
    }

    fn union(&mut self, first: i32, second: i32) {
        let (a, b) = (self.find(first), self.find(second));
        if a != b {
            self.parent.insert(b, a);
        }
    }

    fn roots(&mut self) -> BTreeSet<i32> {
        let layers: Vec<i32> = self.parent.keys().copied().collect();
        layers.into_iter().map(|l| self.find(l)).collect()
    }

    fn groups_covered_by(&mut self, first_layer: i32, last_layer: i32) -> BTreeSet<i32> {
        let layers: Vec<i32> = self
            .parent
            .keys()
            .copied()
            .filter(|l| *l >= first_layer && *l <= last_layer)
            .collect();
        layers.into_iter().map(|l| self.find(l)).collect()
    }
}

fn terminals(board: &BasicBoard, net_number: i32) -> Vec<Terminal> {
    let mut result = Vec::new();
    for id in board.net_terminal_items(net_number) {
        let Some(item) = board.item(id) else {
            continue;
        };
        let mut signal_layers = BTreeSet::new();
        let mut add = |layer: i32| {
            if layer >= 0
                && (layer as usize) < board.layer_structure.layers.len()
                && board.layer_structure.layers[layer as usize].is_signal
            {
                signal_layers.insert(layer);
            }
        };
        let (x, y) = match &item.kind {
            ItemKind::Pin(pin) => {
                let drill = &pin.drill;
                for layer in drill.first_layer..=drill.last_layer {
                    let index = (layer - drill.first_layer) as usize;
                    if drill.shapes.get(index).is_some_and(Option::is_some) {
                        add(layer);
                    }
                }
                (f64::from(drill.center.x), f64::from(drill.center.y))
            }
            ItemKind::Area(area) if area.is_conduction_area() => {
                let center = area.area().get_border().centre_of_gravity();
                add(area.layer());
                (center.x, center.y)
            }
            _ => continue,
        };
        if !signal_layers.is_empty() {
            result.push(Terminal {
                x,
                y,
                signal_layers,
            });
        }
    }
    result
}

/// Prim's algorithm on the Manhattan distance: the tree length and the
/// number of edges that need a bend.
fn mst(terminals: &[Terminal]) -> (f64, i32) {
    let n = terminals.len();
    let mut used = vec![false; n];
    let mut distances = vec![f64::INFINITY; n];
    let mut parents: Vec<Option<usize>> = vec![None; n];
    distances[0] = 0.0;
    let mut length = 0.0;
    let mut bend_count = 0;
    for _ in 0..n {
        let mut current: Option<usize> = None;
        for index in 0..n {
            if !used[index] && current.is_none_or(|c| distances[index] < distances[c]) {
                current = Some(index);
            }
        }
        let Some(current) = current else {
            break;
        };
        used[current] = true;
        if let Some(parent) = parents[current] {
            let (from, to) = (&terminals[parent], &terminals[current]);
            length += distances[current];
            if from.x != to.x && from.y != to.y {
                bend_count += 1;
            }
        }
        for index in 0..n {
            if !used[index] {
                let (a, b) = (&terminals[current], &terminals[index]);
                let distance = (a.x - b.x).abs() + (a.y - b.y).abs();
                if distance < distances[index] {
                    distances[index] = distance;
                    parents[index] = Some(current);
                }
            }
        }
    }
    (length, bend_count)
}

/// The vias needed to join the layer groups of the terminals, choosing
/// greedily the via of the net's via rule that joins the most groups.
fn minimum_via_count(board: &BasicBoard, net_number: i32, terminals: &[Terminal]) -> i32 {
    let mut groups = LayerGroups::default();
    for terminal in terminals {
        groups.add_layers(&terminal.signal_layers);
    }
    let mut remaining = groups.roots();
    if remaining.len() <= 1 {
        return 0;
    }
    let Some(via_rule) = board
        .rules
        .net_class_of(net_number)
        .and_then(|c| c.via_rule())
    else {
        return 0;
    };
    let spans: Vec<(i32, i32)> = board
        .rules
        .via_rules
        .get(via_rule)
        .vias()
        .iter()
        .filter_map(|v| {
            board
                .library
                .padstacks
                .get(board.rules.via_infos.get(*v).padstack)
        })
        .map(|p| {
            let (from, to) = (p.from_layer(), p.to_layer());
            (from.min(to), from.max(to))
        })
        .collect();
    let mut via_count = 0;
    while !remaining.is_empty() {
        let mut best: Option<BTreeSet<i32>> = None;
        for &(first, last) in &spans {
            let covered: BTreeSet<i32> = groups
                .groups_covered_by(first, last)
                .intersection(&remaining)
                .copied()
                .collect();
            if covered.len() > best.as_ref().map_or(0, BTreeSet::len) {
                best = Some(covered);
            }
        }
        let Some(best) = best.filter(|b| !b.is_empty()) else {
            break;
        };
        remaining.retain(|g| !best.contains(g));
        via_count += 1;
    }
    via_count
}

/// Calculates the routing lower bounds of `board`.
///
/// Port of `BoardStatisticsBoundsCalculator.calculate` (without its cache).
pub fn calculate_bounds(board: &BasicBoard) -> BoardStatisticsBounds {
    let resolution = board.communication.resolution;
    let to_mm = Unit::scale(1.0, board.communication.unit, Unit::Mm)
        / f64::from(if resolution > 0 { resolution } else { 1 });
    let mut min_trace_length = 0.0;
    let mut min_via_count = 0;
    let mut min_bend_count = 0;
    for net_number in 1..=board.rules.nets.max_net_number() {
        if board.rules.nets.get(net_number).is_none() {
            continue;
        }
        let terminals = terminals(board, net_number);
        if terminals.len() < 2 {
            continue;
        }
        let (length, bends) = mst(&terminals);
        min_trace_length += length * to_mm;
        min_bend_count += bends;
        min_via_count += minimum_via_count(board, net_number, &terminals);
    }
    BoardStatisticsBounds {
        min_trace_length_mm: min_trace_length as f32,
        min_via_count,
        min_bend_count,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn terminal(x: f64, y: f64) -> Terminal {
        Terminal {
            x,
            y,
            signal_layers: BTreeSet::from([0]),
        }
    }

    #[test]
    fn mst_uses_manhattan_distance_and_counts_diagonal_edges() {
        let terminals = [
            terminal(0.0, 0.0),
            terminal(10.0, 0.0),
            terminal(10.0, 5.0),
            terminal(0.0, 20.0),
        ];
        let (length, bends) = mst(&terminals);
        // 0-1 (10), 1-2 (5), then 3 is closest to 0 (20).
        assert_eq!(length, 35.0);
        assert_eq!(bends, 0);
        let (length, bends) = mst(&[terminal(0.0, 0.0), terminal(3.0, 4.0)]);
        assert_eq!(length, 7.0);
        assert_eq!(bends, 1);
    }

    #[test]
    fn layer_groups_join_layers_of_one_terminal() {
        let mut groups = LayerGroups::default();
        groups.add_layers(&BTreeSet::from([0, 3]));
        groups.add_layers(&BTreeSet::from([1]));
        assert_eq!(groups.roots().len(), 2);
        assert_eq!(groups.groups_covered_by(0, 1).len(), 2);
        assert_eq!(groups.groups_covered_by(3, 3).len(), 1);
    }
}
