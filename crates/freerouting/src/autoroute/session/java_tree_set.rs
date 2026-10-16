//! A `java.util.TreeSet` that makes `TreeMap.put`'s comparisons (A07.3).
//!
//! Port of `java.util.TreeMap.put(K, V, boolean)`, `addEntryToEmptyMap`,
//! `addEntry`, `fixAfterInsertion`, `rotateLeft` and `rotateRight` (JDK 25,
//! the runtime of the Java reference harness), with the comparator as a
//! closure. The set compares the new element with the same nodes, in the
//! same order, as Java's red-black tree does, and rebalances the same way,
//! so a comparator that is **not transitive** gives the same contents and
//! the same iteration order as Java.
//!
//! Only the any-angle neighbour set needs it
//! (`SortedRoomNeighbours.SortedRoomNeighbour.compareTo`, whose distance
//! tolerance breaks transitivity). Every other set of the router has a
//! total order and uses `BTreeSet` or a sorted vector.
//!
//! `insert` returns false when the comparator returns 0 for a node: as
//! `TreeSet.add`, the element already there is kept and the new one dropped.

use std::cmp::Ordering;

/// A node of the tree; links are arena indexes.
#[derive(Clone, Debug)]
struct Node<T> {
    value: T,
    left: Option<usize>,
    right: Option<usize>,
    parent: Option<usize>,
    /// `TreeMap.Entry.color`: true for `BLACK`.
    black: bool,
}

/// A red-black tree set ordered by `compare(new, existing)`, Java's
/// `TreeSet` with a comparator.
pub struct JavaTreeSet<T, F>
where
    F: FnMut(&T, &T) -> Ordering,
{
    nodes: Vec<Node<T>>,
    root: Option<usize>,
    compare: F,
}

impl<T, F> JavaTreeSet<T, F>
where
    F: FnMut(&T, &T) -> Ordering,
{
    /// An empty set (`new TreeSet<>(comparator)`).
    pub fn new(compare: F) -> JavaTreeSet<T, F> {
        JavaTreeSet {
            nodes: Vec::new(),
            root: None,
            compare,
        }
    }

    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// `TreeSet.add`: true if `value` was added, false if the comparator
    /// found an equal element (which stays).
    pub fn insert(&mut self, value: T) -> bool {
        let Some(mut t) = self.root else {
            // PARITY: TreeMap.addEntryToEmptyMap compares the key with
            // itself (a type check) before it becomes the root.
            (self.compare)(&value, &value);
            self.nodes.push(Node {
                value,
                left: None,
                right: None,
                parent: None,
                black: true,
            });
            self.root = Some(0);
            return true;
        };
        // PARITY: TreeMap.put, `cmp = cpr.compare(key, t.key)` from the root
        // down; a 0 keeps the old key.
        let (parent, less) = loop {
            let cmp = (self.compare)(&value, &self.nodes[t].value);
            let next = match cmp {
                Ordering::Less => self.nodes[t].left,
                Ordering::Greater => self.nodes[t].right,
                Ordering::Equal => return false,
            };
            match next {
                Some(n) => t = n,
                None => break (t, cmp == Ordering::Less),
            }
        };
        let id = self.nodes.len();
        self.nodes.push(Node {
            value,
            left: None,
            right: None,
            parent: Some(parent),
            black: true,
        });
        if less {
            self.nodes[parent].left = Some(id);
        } else {
            self.nodes[parent].right = Some(id);
        }
        self.fix_after_insertion(id);
        true
    }

    /// The elements in order (`TreeSet.iterator`).
    pub fn iter(&self) -> Iter<'_, T, F> {
        Iter {
            set: self,
            next: self.root.map(|r| self.leftmost(r)),
        }
    }

    /// `TreeSet.first`.
    pub fn first(&self) -> Option<&T> {
        self.root.map(|r| &self.nodes[self.leftmost(r)].value)
    }

    /// `TreeSet.last` (`getLast`).
    pub fn last(&self) -> Option<&T> {
        let mut n = self.root?;
        while let Some(r) = self.nodes[n].right {
            n = r;
        }
        Some(&self.nodes[n].value)
    }

    /// The elements in order, moved out.
    pub fn into_sorted_vec(self) -> Vec<T> {
        let order: Vec<usize> = {
            let mut order = Vec::with_capacity(self.nodes.len());
            let mut next = self.root.map(|r| self.leftmost(r));
            while let Some(n) = next {
                order.push(n);
                next = self.successor(n);
            }
            order
        };
        let mut slots: Vec<Option<T>> = self.nodes.into_iter().map(|n| Some(n.value)).collect();
        order
            .into_iter()
            .map(|n| slots[n].take().expect("each node once"))
            .collect()
    }

    fn leftmost(&self, mut n: usize) -> usize {
        while let Some(l) = self.nodes[n].left {
            n = l;
        }
        n
    }

    /// `TreeMap.successor`.
    fn successor(&self, n: usize) -> Option<usize> {
        if let Some(r) = self.nodes[n].right {
            return Some(self.leftmost(r));
        }
        let mut child = n;
        let mut p = self.nodes[n].parent;
        while let Some(parent) = p {
            if self.nodes[parent].right != Some(child) {
                break;
            }
            child = parent;
            p = self.nodes[parent].parent;
        }
        p
    }

    fn parent_of(&self, n: Option<usize>) -> Option<usize> {
        n.and_then(|n| self.nodes[n].parent)
    }

    fn left_of(&self, n: Option<usize>) -> Option<usize> {
        n.and_then(|n| self.nodes[n].left)
    }

    fn right_of(&self, n: Option<usize>) -> Option<usize> {
        n.and_then(|n| self.nodes[n].right)
    }

    /// `colorOf`: a missing node is black.
    fn is_black(&self, n: Option<usize>) -> bool {
        n.is_none_or(|n| self.nodes[n].black)
    }

    fn set_black(&mut self, n: Option<usize>, black: bool) {
        if let Some(n) = n {
            self.nodes[n].black = black;
        }
    }

    /// `TreeMap.rotateLeft`.
    fn rotate_left(&mut self, p: Option<usize>) {
        let Some(p) = p else { return };
        let r = self.nodes[p].right.expect("rotateLeft has a right child");
        self.nodes[p].right = self.nodes[r].left;
        if let Some(rl) = self.nodes[r].left {
            self.nodes[rl].parent = Some(p);
        }
        self.nodes[r].parent = self.nodes[p].parent;
        match self.nodes[p].parent {
            None => self.root = Some(r),
            Some(pp) if self.nodes[pp].left == Some(p) => self.nodes[pp].left = Some(r),
            Some(pp) => self.nodes[pp].right = Some(r),
        }
        self.nodes[r].left = Some(p);
        self.nodes[p].parent = Some(r);
    }

    /// `TreeMap.rotateRight`.
    fn rotate_right(&mut self, p: Option<usize>) {
        let Some(p) = p else { return };
        let l = self.nodes[p].left.expect("rotateRight has a left child");
        self.nodes[p].left = self.nodes[l].right;
        if let Some(lr) = self.nodes[l].right {
            self.nodes[lr].parent = Some(p);
        }
        self.nodes[l].parent = self.nodes[p].parent;
        match self.nodes[p].parent {
            None => self.root = Some(l),
            Some(pp) if self.nodes[pp].right == Some(p) => self.nodes[pp].right = Some(l),
            Some(pp) => self.nodes[pp].left = Some(l),
        }
        self.nodes[l].right = Some(p);
        self.nodes[p].parent = Some(l);
    }

    /// `TreeMap.fixAfterInsertion` (CLR).
    fn fix_after_insertion(&mut self, x: usize) {
        self.nodes[x].black = false;
        let mut x = Some(x);
        while let Some(n) = x {
            if Some(n) == self.root || self.is_black(self.nodes[n].parent) {
                break;
            }
            let parent = self.parent_of(x);
            let grand = self.parent_of(parent);
            if parent == self.left_of(grand) {
                let y = self.right_of(grand);
                if !self.is_black(y) {
                    self.set_black(parent, true);
                    self.set_black(y, true);
                    self.set_black(grand, false);
                    x = grand;
                } else {
                    if x == self.right_of(parent) {
                        x = parent;
                        self.rotate_left(x);
                    }
                    let parent = self.parent_of(x);
                    self.set_black(parent, true);
                    let grand = self.parent_of(parent);
                    self.set_black(grand, false);
                    self.rotate_right(grand);
                }
            } else {
                let y = self.left_of(grand);
                if !self.is_black(y) {
                    self.set_black(parent, true);
                    self.set_black(y, true);
                    self.set_black(grand, false);
                    x = grand;
                } else {
                    if x == self.left_of(parent) {
                        x = parent;
                        self.rotate_right(x);
                    }
                    let parent = self.parent_of(x);
                    self.set_black(parent, true);
                    let grand = self.parent_of(parent);
                    self.set_black(grand, false);
                    self.rotate_left(grand);
                }
            }
        }
        let root = self.root;
        self.set_black(root, true);
    }
}

/// The in-order iterator of a [`JavaTreeSet`].
pub struct Iter<'a, T, F>
where
    F: FnMut(&T, &T) -> Ordering,
{
    set: &'a JavaTreeSet<T, F>,
    next: Option<usize>,
}

impl<'a, T, F> Iterator for Iter<'a, T, F>
where
    F: FnMut(&T, &T) -> Ordering,
{
    type Item = &'a T;

    fn next(&mut self) -> Option<&'a T> {
        let n = self.next?;
        self.next = self.set.successor(n);
        Some(&self.set.nodes[n].value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The red-black invariants: a black root, no red node with a red
    /// child, the same black count on every path; and the parent links.
    fn check<T, F: FnMut(&T, &T) -> Ordering>(set: &JavaTreeSet<T, F>) {
        fn black_height<T, F: FnMut(&T, &T) -> Ordering>(
            set: &JavaTreeSet<T, F>,
            n: Option<usize>,
            parent: Option<usize>,
        ) -> usize {
            let Some(n) = n else { return 1 };
            let node = &set.nodes[n];
            assert_eq!(node.parent, parent, "parent link of {n}");
            if !node.black {
                assert!(set.is_black(node.left) && set.is_black(node.right));
            }
            let l = black_height(set, node.left, Some(n));
            let r = black_height(set, node.right, Some(n));
            assert_eq!(l, r, "black height at {n}");
            l + usize::from(node.black)
        }
        assert!(set.is_black(set.root));
        black_height(set, set.root, None);
    }

    #[test]
    fn a_total_order_sorts_and_drops_duplicates() {
        let mut set = JavaTreeSet::new(|a: &i32, b: &i32| a.cmp(b));
        let mut added = Vec::new();
        for v in [5, 3, 8, 1, 4, 7, 9, 2, 6, 5, 3, 10, 0] {
            added.push(set.insert(v));
            check(&set);
        }
        assert_eq!(
            added,
            [
                true, true, true, true, true, true, true, true, true, false, false, true, true
            ]
        );
        assert_eq!(
            set.iter().copied().collect::<Vec<_>>(),
            (0..=10).collect::<Vec<_>>()
        );
        assert_eq!(set.first(), Some(&0));
        assert_eq!(set.last(), Some(&10));
        assert_eq!(set.len(), 11);
        assert_eq!(set.into_sorted_vec(), (0..=10).collect::<Vec<_>>());
    }

    #[test]
    fn ascending_and_descending_inserts_stay_balanced() {
        let mut up = JavaTreeSet::new(|a: &i32, b: &i32| a.cmp(b));
        let mut down = JavaTreeSet::new(|a: &i32, b: &i32| a.cmp(b));
        for v in 0..200 {
            up.insert(v);
            down.insert(-v);
        }
        check(&up);
        check(&down);
        assert_eq!(up.iter().count(), 200);
        assert_eq!(down.first(), Some(&-199));
    }

    /// The first insert compares the element with itself, as
    /// `TreeMap.addEntryToEmptyMap` does.
    #[test]
    fn the_first_insert_compares_the_element_with_itself() {
        let mut calls = Vec::new();
        {
            let mut set = JavaTreeSet::new(|a: &i32, b: &i32| {
                calls.push((*a, *b));
                a.cmp(b)
            });
            set.insert(4);
            set.insert(2);
        }
        assert_eq!(calls, [(4, 4), (2, 4)]);
    }

    #[test]
    fn an_empty_set_has_no_ends() {
        let set = JavaTreeSet::new(|a: &i32, b: &i32| a.cmp(b));
        assert!(set.is_empty());
        assert_eq!(set.first(), None);
        assert_eq!(set.last(), None);
        assert_eq!(set.iter().next(), None);
    }
}
