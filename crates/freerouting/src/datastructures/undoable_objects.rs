use std::collections::BTreeMap;
use std::sync::Arc;

/// Index of a version node in the node arena.
type NodeIdx = usize;

/// One version of an object.
#[derive(Clone, Debug)]
struct Node<K, V> {
    key: K,
    /// Shared with the snapshots and undo versions that hold the same
    /// version; changed through `Arc::make_mut` (copy on write).
    object: Arc<V>,
    /// The undo stack level this version was created at.
    level: usize,
    /// The version to restore on undo.
    undo: Option<NodeIdx>,
    /// The version to restore on redo.
    redo: Option<NodeIdx>,
}

/// One step of [`UndoableObjects::walk_state`].
pub(crate) enum StateRecord<'a, K, V> {
    Header {
        stack_level: usize,
        redo_possible: bool,
    },
    /// A key of the map; its node follows.
    Key(&'a K),
    /// A version node at its first visit; its undo, then its redo link
    /// follow.
    Node { level: usize, object: &'a V },
    /// A node visited before, by its handle (the order of first visits).
    Seen(usize),
    /// A missing undo or redo link.
    NoNode,
    /// The start of a level of the deleted-objects stack, with its length.
    DeletedLevel(usize),
}

/// A set of objects that records its changes so that they can be undone and
/// redone, snapshot by snapshot.
///
/// Port of `datastructures.UndoableObjects`. Java keys the map by the object
/// itself (through `compareTo`); here the caller supplies the key `K`
/// explicitly, and iteration follows the order of `K`, which the caller picks to
/// match the Java `compareTo` (see `ItemId`). The version nodes live in an
/// arena instead of being linked by references. Nodes that nothing refers to any
/// more (Java leaves them to the garbage collector) are reclaimed by an
/// occasional mark-and-sweep pass over the arena.
///
/// The object stored in the current node is the live object: callers mutate it
/// through [`UndoableObjects::get_mut`] after calling
/// [`UndoableObjects::save_for_undo`], the same way Java mutates the object
/// after `saveForUndo` stored a clone of it.
#[derive(Clone, Debug)]
pub struct UndoableObjects<K: Ord + Clone, V: Clone> {
    objects: BTreeMap<K, NodeIdx>,
    nodes: Vec<Option<Node<K, V>>>,
    free: Vec<NodeIdx>,
    deleted_objects_stack: Vec<Vec<NodeIdx>>,
    stack_level: usize,
    redo_possible: bool,
    gc_threshold: usize,
}

impl<K: Ord + Clone, V: Clone> Default for UndoableObjects<K, V> {
    fn default() -> Self {
        Self::new()
    }
}

impl<K: Ord + Clone, V: Clone> UndoableObjects<K, V> {
    const MIN_GC_THRESHOLD: usize = 1024;

    pub fn new() -> Self {
        UndoableObjects {
            objects: BTreeMap::new(),
            nodes: Vec::new(),
            free: Vec::new(),
            deleted_objects_stack: Vec::new(),
            stack_level: 0,
            redo_possible: false,
            gc_threshold: Self::MIN_GC_THRESHOLD,
        }
    }

    fn node(&self, idx: NodeIdx) -> &Node<K, V> {
        self.nodes[idx].as_ref().expect("live undo node")
    }

    fn node_mut(&mut self, idx: NodeIdx) -> &mut Node<K, V> {
        self.nodes[idx].as_mut().expect("live undo node")
    }

    fn alloc(&mut self, node: Node<K, V>) -> NodeIdx {
        if let Some(idx) = self.free.pop() {
            self.nodes[idx] = Some(node);
            idx
        } else {
            self.nodes.push(Some(node));
            self.nodes.len() - 1
        }
    }

    /// The current undo stack level (the number of snapshots).
    pub fn stack_level(&self) -> usize {
        self.stack_level
    }

    /// Iterates the visible objects in key order (Java `startReadObject` and
    /// `readObject`, which skip versions above the current stack level).
    pub fn iter(&self) -> impl Iterator<Item = (&K, &V)> + '_ {
        self.objects.iter().filter_map(move |(key, &idx)| {
            let node = self.node(idx);
            (node.level <= self.stack_level).then_some((key, &*node.object))
        })
    }

    /// The keys of the visible objects in key order.
    pub fn keys(&self) -> impl Iterator<Item = &K> + '_ {
        self.iter().map(|(key, _)| key)
    }

    /// The number of visible objects.
    pub fn len(&self) -> usize {
        self.iter().count()
    }

    pub fn is_empty(&self) -> bool {
        self.iter().next().is_none()
    }

    /// The current version of the object with key `key`, if it is visible.
    pub fn get(&self, key: &K) -> Option<&V> {
        let node = self.node(*self.objects.get(key)?);
        (node.level <= self.stack_level).then_some(&*node.object)
    }

    /// The current version of the object with key `key` for changing it in
    /// place. Call [`save_for_undo`](Self::save_for_undo) first if the change
    /// should be undoable.
    pub fn get_mut(&mut self, key: &K) -> Option<&mut V> {
        let idx = *self.objects.get(key)?;
        let level = self.stack_level;
        let node = self.node_mut(idx);
        (node.level <= level).then_some(Arc::make_mut(&mut node.object))
    }

    /// True if an object with key `key` is visible.
    pub fn contains(&self, key: &K) -> bool {
        self.get(key).is_some()
    }

    /// Adds an object. An existing object with the same key is replaced, as in
    /// Java's map `put`.
    pub fn insert(&mut self, key: K, object: V) {
        self.disable_redo();
        let idx = self.alloc(Node {
            key: key.clone(),
            object: Arc::new(object),
            level: self.stack_level,
            undo: None,
            redo: None,
        });
        self.objects.insert(key, idx);
        self.maybe_collect_garbage();
    }

    /// Removes the object with key `key`, keeping it for undo. Returns false if
    /// it was not found.
    pub fn delete(&mut self, key: &K) -> bool {
        self.disable_redo();
        let Some(&idx) = self.objects.get(key) else {
            return false;
        };
        if let Some(delete_list_level) = self.deleted_objects_stack.len().checked_sub(1) {
            let node = self.node(idx);
            let to_keep = if node.level < self.stack_level {
                Some(idx)
            } else {
                node.undo
            };
            if let Some(keep) = to_keep {
                self.deleted_objects_stack[delete_list_level].push(keep);
            }
        }
        self.objects.remove(key);
        true
    }

    /// Starts a new undo level: changes from now on are undone together.
    pub fn generate_snapshot(&mut self) {
        self.disable_redo();
        self.deleted_objects_stack.push(Vec::new());
        self.stack_level += 1;
    }

    /// Restores the state of the previous snapshot. Returns the keys of the
    /// objects whose current version was cancelled and the keys of the objects
    /// whose earlier version was restored, in Java's order, or `None` if there
    /// is nothing to undo.
    pub fn undo(&mut self) -> Option<(Vec<K>, Vec<K>)> {
        self.undo_with(|_| {})
    }

    /// [`undo`](Self::undo), calling `on_cancelled` with each cancelled
    /// version (Java passes the cancelled objects themselves, and the board
    /// changes them: it takes them off the search trees).
    pub fn undo_with(&mut self, mut on_cancelled: impl FnMut(&mut V)) -> Option<(Vec<K>, Vec<K>)> {
        if self.stack_level == 0 {
            return None;
        }
        let mut cancelled = Vec::new();
        let mut restored = Vec::new();
        let entries: Vec<(K, NodeIdx)> = self
            .objects
            .iter()
            .map(|(key, &idx)| (key.clone(), idx))
            .collect();
        for (key, idx) in entries {
            let node = self.node(idx);
            if node.level == self.stack_level {
                if let Some(undo_idx) = node.undo {
                    self.node_mut(undo_idx).redo = Some(idx);
                    self.objects.insert(key.clone(), undo_idx);
                    restored.push(self.node(undo_idx).key.clone());
                }
                on_cancelled(Arc::make_mut(&mut self.node_mut(idx).object));
                cancelled.push(key);
            }
        }
        let delete_list = self.deleted_objects_stack[self.stack_level - 1].clone();
        for deleted in delete_list {
            let key = self.node(deleted).key.clone();
            self.objects.insert(key.clone(), deleted);
            restored.push(key);
        }
        self.stack_level -= 1;
        self.redo_possible = true;
        Some((cancelled, restored))
    }

    /// Restores the state before the last undo. Returns the cancelled and the
    /// restored keys as [`undo`](Self::undo) does, or `None` if there is
    /// nothing to redo.
    pub fn redo(&mut self) -> Option<(Vec<K>, Vec<K>)> {
        self.redo_with(|_| {})
    }

    /// [`redo`](Self::redo), calling `on_cancelled` with each cancelled
    /// version.
    pub fn redo_with(&mut self, mut on_cancelled: impl FnMut(&mut V)) -> Option<(Vec<K>, Vec<K>)> {
        if self.stack_level >= self.deleted_objects_stack.len() {
            return None;
        }
        self.stack_level += 1;
        let mut cancelled = Vec::new();
        let mut restored = Vec::new();
        let entries: Vec<(K, NodeIdx)> = self
            .objects
            .iter()
            .map(|(key, &idx)| (key.clone(), idx))
            .collect();
        for (key, idx) in entries {
            let node = self.node(idx);
            match node.redo {
                Some(redo_idx) if self.node(redo_idx).level == self.stack_level => {
                    self.objects.insert(key.clone(), redo_idx);
                    on_cancelled(Arc::make_mut(&mut self.node_mut(idx).object));
                    cancelled.push(key);
                    restored.push(self.node(redo_idx).key.clone());
                }
                _ => {
                    if node.level == self.stack_level {
                        restored.push(key);
                    }
                }
            }
        }
        let delete_list = self.deleted_objects_stack[self.stack_level - 1].clone();
        for deleted in delete_list {
            let mut current = deleted;
            while let Some(redo_idx) = self.node(current).redo {
                if self.node(redo_idx).level > self.stack_level {
                    break;
                }
                current = redo_idx;
            }
            let key = self.node(current).key.clone();
            self.objects.remove(&key);
            if let Some(pos) = restored.iter().position(|k| *k == key) {
                restored.remove(pos);
            } else {
                on_cancelled(Arc::make_mut(&mut self.node_mut(current).object));
                cancelled.push(key);
            }
        }
        Some((cancelled, restored))
    }

    /// Merges the last snapshot into the previous one. Returns false if there
    /// is no snapshot.
    pub fn pop_snapshot(&mut self) -> bool {
        self.disable_redo();
        if self.stack_level == 0 {
            return false;
        }
        let indices: Vec<NodeIdx> = self.objects.values().copied().collect();
        for idx in indices {
            let node = self.node(idx);
            if node.level + 1 == self.stack_level {
                if let Some(redo_idx) = node.redo {
                    if self.node(redo_idx).level == self.stack_level {
                        let undo_idx = node.undo;
                        self.node_mut(redo_idx).undo = undo_idx;
                        if let Some(undo_idx) = undo_idx {
                            self.node_mut(undo_idx).redo = Some(redo_idx);
                        }
                    }
                }
            } else if node.level >= self.stack_level {
                self.node_mut(idx).level -= 1;
            }
        }
        let size = self.deleted_objects_stack.len();
        if size >= 2 {
            let from = self.deleted_objects_stack[size - 1].clone();
            for deleted in from {
                let node = self.node(deleted);
                let to_keep = if node.level + 1 < self.stack_level {
                    Some(deleted)
                } else {
                    node.undo
                };
                if let Some(keep) = to_keep {
                    self.deleted_objects_stack[size - 2].push(keep);
                }
            }
        }
        self.deleted_objects_stack.pop();
        self.stack_level -= 1;
        true
    }

    /// Keeps a copy of the current version of the object with key `key`, so
    /// that a following change can be undone. Does nothing if the object was
    /// already saved at this level. Returns false if the object was not found.
    pub fn save_for_undo(&mut self, key: &K) -> bool {
        self.disable_redo();
        let Some(&idx) = self.objects.get(key) else {
            return false;
        };
        let node = self.node(idx);
        if node.level < self.stack_level {
            let old = Node {
                key: node.key.clone(),
                object: Arc::clone(&node.object),
                level: node.level,
                undo: node.undo,
                redo: Some(idx),
            };
            let old_idx = self.alloc(old);
            let level = self.stack_level;
            let node = self.node_mut(idx);
            node.undo = Some(old_idx);
            node.level = level;
            self.maybe_collect_garbage();
        }
        true
    }

    fn disable_redo(&mut self) {
        if !self.redo_possible {
            return;
        }
        self.redo_possible = false;
        self.deleted_objects_stack.truncate(self.stack_level);
        let level = self.stack_level;
        let entries: Vec<(K, NodeIdx)> = self
            .objects
            .iter()
            .map(|(key, &idx)| (key.clone(), idx))
            .collect();
        for (key, idx) in entries {
            let node_level = self.node(idx).level;
            if node_level > level {
                self.objects.remove(&key);
            } else if node_level == level {
                self.node_mut(idx).redo = None;
            }
        }
    }

    fn maybe_collect_garbage(&mut self) {
        let live = self.nodes.len() - self.free.len();
        if live < self.gc_threshold {
            return;
        }
        self.collect_garbage();
        let live = self.nodes.len() - self.free.len();
        self.gc_threshold = (2 * live).max(Self::MIN_GC_THRESHOLD);
    }

    /// Frees the version nodes that are no longer reachable from the current
    /// objects or the delete lists.
    fn collect_garbage(&mut self) {
        let mut marked = vec![false; self.nodes.len()];
        let mut stack: Vec<NodeIdx> = self.objects.values().copied().collect();
        for list in &self.deleted_objects_stack {
            stack.extend(list.iter().copied());
        }
        while let Some(idx) = stack.pop() {
            if marked[idx] {
                continue;
            }
            marked[idx] = true;
            let node = self.node(idx);
            stack.extend(node.undo);
            stack.extend(node.redo);
        }
        for (idx, mark) in marked.into_iter().enumerate() {
            if !mark && self.nodes[idx].is_some() {
                self.nodes[idx] = None;
                self.free.push(idx);
            }
        }
    }

    /// Walks everything Java serializes of this structure (B01.5): the stack
    /// level and `redo_possible`, then each key of the map in key order with
    /// its version node, then the deleted-objects stack level by level, in
    /// list order. A node is reported once, at its first visit, followed by
    /// its undo and redo links; later visits report its handle, so the walk
    /// encodes the shape of the version graph the way Java's back
    /// references do.
    pub(crate) fn walk_state(&self, mut visit: impl FnMut(StateRecord<'_, K, V>)) {
        visit(StateRecord::Header {
            stack_level: self.stack_level,
            redo_possible: self.redo_possible,
        });
        let mut handles: Vec<Option<usize>> = vec![None; self.nodes.len()];
        let mut next_handle = 0;
        for (key, &idx) in &self.objects {
            visit(StateRecord::Key(key));
            self.walk_node(Some(idx), &mut handles, &mut next_handle, &mut visit);
        }
        for list in &self.deleted_objects_stack {
            visit(StateRecord::DeletedLevel(list.len()));
            for &idx in list {
                self.walk_node(Some(idx), &mut handles, &mut next_handle, &mut visit);
            }
        }
    }

    fn walk_node(
        &self,
        start: Option<NodeIdx>,
        handles: &mut [Option<usize>],
        next_handle: &mut usize,
        visit: &mut impl FnMut(StateRecord<'_, K, V>),
    ) {
        // An explicit stack: undo chains can be long.
        let mut stack = vec![start];
        while let Some(entry) = stack.pop() {
            let Some(idx) = entry else {
                visit(StateRecord::NoNode);
                continue;
            };
            if let Some(handle) = handles[idx] {
                visit(StateRecord::Seen(handle));
                continue;
            }
            handles[idx] = Some(*next_handle);
            *next_handle += 1;
            let node = self.node(idx);
            visit(StateRecord::Node {
                level: node.level,
                object: &*node.object,
            });
            stack.push(node.redo);
            stack.push(node.undo);
        }
    }

    /// The number of version nodes held, for tests.
    #[cfg(test)]
    fn node_count(&self) -> usize {
        self.nodes.len() - self.free.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn values(objects: &UndoableObjects<i32, &'static str>) -> Vec<(i32, &'static str)> {
        objects.iter().map(|(k, v)| (*k, *v)).collect()
    }

    #[test]
    fn undo_restores_deleted_and_changed_objects() {
        let mut objects = UndoableObjects::new();
        objects.insert(1, "a");
        objects.insert(2, "b");
        objects.generate_snapshot();
        objects.delete(&1);
        objects.save_for_undo(&2);
        *objects.get_mut(&2).unwrap() = "b2";
        objects.insert(3, "c");
        assert_eq!(values(&objects), vec![(2, "b2"), (3, "c")]);

        let (cancelled, restored) = objects.undo().unwrap();
        assert_eq!(cancelled, vec![2, 3]);
        assert_eq!(restored, vec![2, 1]);
        assert_eq!(values(&objects), vec![(1, "a"), (2, "b")]);

        let (cancelled, restored) = objects.redo().unwrap();
        assert_eq!(values(&objects), vec![(2, "b2"), (3, "c")]);
        assert_eq!(cancelled, vec![2, 1]);
        assert_eq!(restored, vec![2, 3]);
    }

    #[test]
    fn nothing_to_undo_without_snapshot() {
        let mut objects: UndoableObjects<i32, i32> = UndoableObjects::new();
        objects.insert(1, 1);
        assert!(objects.undo().is_none());
        assert!(objects.redo().is_none());
        assert!(!objects.pop_snapshot());
    }

    #[test]
    fn change_after_undo_disables_redo() {
        let mut objects = UndoableObjects::new();
        objects.insert(1, "a");
        objects.generate_snapshot();
        objects.insert(2, "b");
        objects.undo().unwrap();
        objects.insert(3, "c");
        assert!(objects.redo().is_none());
        assert_eq!(values(&objects), vec![(1, "a"), (3, "c")]);
    }

    #[test]
    fn pop_snapshot_merges_levels() {
        let mut objects = UndoableObjects::new();
        objects.insert(1, "a");
        objects.generate_snapshot();
        objects.save_for_undo(&1);
        *objects.get_mut(&1).unwrap() = "a1";
        objects.generate_snapshot();
        objects.save_for_undo(&1);
        *objects.get_mut(&1).unwrap() = "a2";
        objects.delete(&1);
        assert!(objects.pop_snapshot());
        assert_eq!(objects.stack_level(), 1);
        objects.undo().unwrap();
        assert_eq!(values(&objects), vec![(1, "a")]);
    }

    #[test]
    fn unreachable_versions_are_collected() {
        let mut objects = UndoableObjects::new();
        for i in 0..5000 {
            objects.insert(i, i);
            objects.delete(&i);
        }
        objects.insert(-1, -1);
        assert!(objects.node_count() < 3000, "{}", objects.node_count());
        assert_eq!(values_i32(&objects), vec![(-1, -1)]);
    }

    fn values_i32(objects: &UndoableObjects<i32, i32>) -> Vec<(i32, i32)> {
        objects.iter().map(|(k, v)| (*k, *v)).collect()
    }
}
