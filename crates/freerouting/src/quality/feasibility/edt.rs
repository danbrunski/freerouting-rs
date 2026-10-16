//! Exact squared Euclidean distance transform with nearest-source labels
//! (Q03.2).
//!
//! Felzenszwalb and Huttenlocher, "Distance Transforms of Sampled
//! Functions" (Theory of Computing, 2012): the lower envelope of parabolas,
//! applied to the rows and then to the columns. All arithmetic is `i64`;
//! the envelope's intersection abscissae are kept as exact fractions, so the
//! result equals brute force bit for bit.
//!
//! Orders that decide results: among equidistant sources the envelope keeps
//! the parabola that came first in index order, so the label is the source
//! with the lowest row-pass column and then the lowest row. Any source at
//! the returned distance is a valid label; tests compare only `d²` exactly.

/// The squared distance of a cell with no source anywhere.
pub const INF: i64 = i64::MAX;

/// The label of a cell with no source anywhere.
pub const NO_SOURCE: u32 = u32::MAX;

/// The largest grid side, so that every intermediate product fits in `i64`.
const MAX_SIDE: usize = 1 << 20;

/// The squared distance (in cells) from each cell of the `nx × ny` grid to
/// the nearest cell where `mask` is true, and the row-major index of that
/// cell. A grid with no source gives [`INF`] and [`NO_SOURCE`] everywhere.
pub fn squared_edt(mask: &[bool], nx: usize, ny: usize) -> (Vec<i64>, Vec<u32>) {
    assert_eq!(mask.len(), nx * ny, "mask size");
    squared_edt_by(nx, ny, |index| mask[index])
}

/// [`squared_edt`] with the mask given as a predicate on the cell index.
pub fn squared_edt_by(
    nx: usize,
    ny: usize,
    is_source: impl Fn(usize) -> bool,
) -> (Vec<i64>, Vec<u32>) {
    assert!(nx <= MAX_SIDE && ny <= MAX_SIDE, "grid too large");
    let n = nx * ny;
    let mut row_d = vec![INF; n];
    let mut row_label = vec![NO_SOURCE; n];
    let mut envelope = Envelope::with_capacity(nx.max(ny));
    let mut f = vec![INF; nx.max(ny)];
    let mut labels = vec![NO_SOURCE; nx.max(ny)];
    let mut out_d = vec![INF; nx.max(ny)];
    let mut out_label = vec![NO_SOURCE; nx.max(ny)];
    for j in 0..ny {
        for i in 0..nx {
            let index = j * nx + i;
            let source = is_source(index);
            f[i] = if source { 0 } else { INF };
            labels[i] = if source { index as u32 } else { NO_SOURCE };
        }
        envelope.transform(
            &f[..nx],
            &labels[..nx],
            &mut out_d[..nx],
            &mut out_label[..nx],
        );
        row_d[j * nx..(j + 1) * nx].copy_from_slice(&out_d[..nx]);
        row_label[j * nx..(j + 1) * nx].copy_from_slice(&out_label[..nx]);
    }
    let mut d = vec![INF; n];
    let mut label = vec![NO_SOURCE; n];
    for i in 0..nx {
        for j in 0..ny {
            f[j] = row_d[j * nx + i];
            labels[j] = row_label[j * nx + i];
        }
        envelope.transform(
            &f[..ny],
            &labels[..ny],
            &mut out_d[..ny],
            &mut out_label[..ny],
        );
        for j in 0..ny {
            d[j * nx + i] = out_d[j];
            label[j * nx + i] = out_label[j];
        }
    }
    (d, label)
}

/// A boundary between two parabolas of the envelope: `num / den` with
/// `den > 0`, or an infinite end.
#[derive(Clone, Copy, Debug)]
enum Bound {
    NegInf,
    At { num: i64, den: i64 },
    PosInf,
}

/// The scratch space of the 1D transform.
struct Envelope {
    sites: Vec<usize>,
    bounds: Vec<Bound>,
}

impl Envelope {
    fn with_capacity(n: usize) -> Self {
        Envelope {
            sites: Vec::with_capacity(n),
            bounds: Vec::with_capacity(n + 1),
        }
    }

    /// The abscissa where the parabola of site `q` overtakes that of `p`
    /// (`p < q`).
    fn intersection(f: &[i64], p: usize, q: usize) -> Bound {
        let (p, q) = (p as i64, q as i64);
        let num = (f[q as usize] + q * q) - (f[p as usize] + p * p);
        Bound::At {
            num,
            den: 2 * (q - p),
        }
    }

    /// `a ≤ b` for a finite `a`.
    fn at_most(a: Bound, b: Bound) -> bool {
        match (a, b) {
            (Bound::At { num: n1, den: d1 }, Bound::At { num: n2, den: d2 }) => {
                i128::from(n1) * i128::from(d2) <= i128::from(n2) * i128::from(d1)
            }
            (_, Bound::NegInf) => false,
            (_, Bound::PosInf) => true,
            (Bound::NegInf, _) => true,
            (Bound::PosInf, _) => false,
        }
    }

    /// `bound < x`.
    fn below(bound: Bound, x: i64) -> bool {
        match bound {
            Bound::NegInf => true,
            Bound::PosInf => false,
            Bound::At { num, den } => num < x * den,
        }
    }

    /// The 1D squared distance transform of `f` (`INF` where there is no
    /// site), carrying the site labels.
    fn transform(&mut self, f: &[i64], labels: &[u32], d: &mut [i64], out: &mut [u32]) {
        self.sites.clear();
        self.bounds.clear();
        for q in 0..f.len() {
            if f[q] == INF {
                continue;
            }
            if self.sites.is_empty() {
                self.sites.push(q);
                self.bounds.push(Bound::NegInf);
                continue;
            }
            let mut s = Self::intersection(f, *self.sites.last().expect("site"), q);
            while Self::at_most(s, *self.bounds.last().expect("bound")) {
                self.sites.pop();
                self.bounds.pop();
                match self.sites.last() {
                    Some(&p) => s = Self::intersection(f, p, q),
                    None => {
                        s = Bound::NegInf;
                        break;
                    }
                }
            }
            self.sites.push(q);
            self.bounds.push(s);
        }
        if self.sites.is_empty() {
            d.fill(INF);
            out.fill(NO_SOURCE);
            return;
        }
        self.bounds.push(Bound::PosInf);
        let mut k = 0;
        for x in 0..f.len() {
            while Self::below(self.bounds[k + 1], x as i64) {
                k += 1;
            }
            let site = self.sites[k];
            let dx = x as i64 - site as i64;
            d[x] = dx * dx + f[site];
            out[x] = labels[site];
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use freerouting_geometry::jmath::JavaRandom;

    fn brute_force(mask: &[bool], nx: usize, ny: usize) -> Vec<i64> {
        let sources: Vec<(i64, i64)> = (0..nx * ny)
            .filter(|&c| mask[c])
            .map(|c| ((c % nx) as i64, (c / nx) as i64))
            .collect();
        (0..nx * ny)
            .map(|c| {
                let (x, y) = ((c % nx) as i64, (c / nx) as i64);
                sources
                    .iter()
                    .map(|(sx, sy)| (x - sx).pow(2) + (y - sy).pow(2))
                    .min()
                    .unwrap_or(INF)
            })
            .collect()
    }

    /// 200 random 64 × 48 masks: `d²` equals brute force exactly, and every
    /// label is a source at that distance.
    #[test]
    fn matches_brute_force_on_random_masks() {
        let (nx, ny) = (64, 48);
        let mut random = JavaRandom::new(20261015);
        for round in 0..200 {
            // Densities from very sparse to dense.
            let density = 1 + random.next_int(60);
            let mask: Vec<bool> = (0..nx * ny)
                .map(|_| random.next_int(1000) < density)
                .collect();
            let (d, label) = squared_edt(&mask, nx, ny);
            assert_eq!(d, brute_force(&mask, nx, ny), "round {round}");
            for c in 0..nx * ny {
                if d[c] == INF {
                    assert_eq!(label[c], NO_SOURCE);
                    continue;
                }
                let s = label[c] as usize;
                assert!(mask[s], "round {round}: label of {c} is no source");
                let (dx, dy) = (
                    (c % nx) as i64 - (s % nx) as i64,
                    (c / nx) as i64 - (s / nx) as i64,
                );
                assert_eq!(dx * dx + dy * dy, d[c], "round {round}: cell {c}");
            }
        }
    }

    #[test]
    fn empty_mask_is_infinite() {
        let (d, label) = squared_edt(&[false; 12], 4, 3);
        assert!(d.iter().all(|v| *v == INF));
        assert!(label.iter().all(|v| *v == NO_SOURCE));
    }

    #[test]
    fn single_source_and_ties() {
        let mut mask = vec![false; 25];
        mask[12] = true;
        let (d, _) = squared_edt(&mask, 5, 5);
        assert_eq!(d[0], 8);
        assert_eq!(d[12], 0);
        assert_eq!(d[14], 4);
        // Two sources equidistant from the middle cell: the first in index
        // order wins.
        let mut mask = vec![false; 5];
        mask[0] = true;
        mask[4] = true;
        let (d, label) = squared_edt(&mask, 5, 1);
        assert_eq!(d, vec![0, 1, 4, 1, 0]);
        assert_eq!(label[2], 0);
    }
}
