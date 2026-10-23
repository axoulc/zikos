//! Sync engine for the shared file tree: the tree-op event envelope that
//! travels over Matrix, canonical ordering, and the deterministic replay
//! with last-writer-wins conflict resolution (losers preserved under
//! `Conflicts/`).
//!
//! Everything here is pure: no I/O, no clocks, no SDK types.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::tree::{FileTree, NodeId, NodeKind, TreeError, TreeOp};

/// The `Conflicts/` folder at the tree root: a fixed well-known id (like
/// `ROOT_ID`) so every device materializes the identical folder. Created
/// lazily, on the first conflict only.
pub const CONFLICTS_ID: NodeId = Uuid::from_u128(1);

/// One preserved conflict loser: the node's state was snapshotted as a
/// copy under `Conflicts/` instead of vanishing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConflictRecord {
    /// Event whose effect lost the race.
    pub losing_event_id: String,
    /// Original node that was snapshotted.
    pub node: NodeId,
    /// The deterministic copy under `Conflicts/`.
    pub copy: NodeId,
}

/// The deterministic product of replaying a set of tree-op events.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReplayOutput {
    /// The materialized tree (may contain a `Conflicts/` folder).
    pub tree: FileTree,
    /// Conflict losers preserved under `Conflicts/`, in replay order.
    pub conflicts: Vec<ConflictRecord>,
    /// Events that could never apply (e.g. ops on nodes a concurrent
    /// delete already removed) — surfaced, never silently dropped.
    pub lost: Vec<TreeOpEvent>,
}

/// Outcome of attempting one event against the current tree state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Outcome {
    /// The op applied cleanly.
    Applied,
    /// The op references something not (yet) present — retry after
    /// later events (handles same-ts event-id inversions).
    Retryable,
    /// The op is unsatisfiable or invalid as written.
    Invalid,
}

/// Deterministic id for a conflict copy: every device derives the same
/// id from the same losing event and original node (uuid v5, pure Rust).
fn conflict_copy_id(event_id: &str, node: NodeId) -> NodeId {
    Uuid::new_v5(
        &Uuid::NAMESPACE_OID,
        format!("zikos.conflict:{event_id}:{node}").as_bytes(),
    )
}

/// Materialize the `Conflicts/` folder at the root if it is not already
/// present. `Ok(false)` means it already existed.
fn ensure_conflicts_folder(tree: &mut FileTree) -> Result<bool, TreeError> {
    if tree.nodes.contains_key(&CONFLICTS_ID) {
        return Ok(false);
    }
    tree.apply(&TreeOp::Create {
        id: CONFLICTS_ID,
        parent: tree.root,
        kind: NodeKind::Folder,
        name: "Conflicts".into(),
    })?;
    Ok(true)
}

/// Preserve a still-unsatisfiable create as a copy under `Conflicts/`
/// rather than dropping it (spec: a conflict shall never lose data
/// silently).
fn orphan_create(tree: &mut FileTree, conflicts: &mut Vec<ConflictRecord>, event: &TreeOpEvent) {
    let TreeOp::Create { id, kind, name, .. } = &event.op else {
        return; // caller only routes creates here
    };
    if ensure_conflicts_folder(tree).is_err() {
        return; // cannot preserve (id collision — practically impossible)
    }
    let copy = conflict_copy_id(&event.event_id, *id);
    if tree
        .apply(&TreeOp::Create {
            id: copy,
            parent: CONFLICTS_ID,
            kind: *kind,
            name: name.clone(),
        })
        .is_ok()
    {
        conflicts.push(ConflictRecord {
            losing_event_id: event.event_id.clone(),
            node: *id,
            copy,
        });
    }
}

/// The replay engine: applies canonical-order events to a [`FileTree`]
/// with last-writer-wins race detection, preserving losers under
/// `Conflicts/`.
struct Engine {
    tree: FileTree,
    conflicts: Vec<ConflictRecord>,
    /// Node → event id of its creation (loser attribution for delete
    /// races against concurrent creations).
    creator: BTreeMap<NodeId, String>,
    /// Node → event id of the last event that changed its name/parent
    /// (loser attribution for rename/move races).
    last_mutator: BTreeMap<NodeId, String>,
}

impl Engine {
    /// Attempt one event; race snapshots happen as side effects before
    /// the winning op applies.
    fn apply_event(&mut self, event: &TreeOpEvent) -> Outcome {
        match &event.op {
            TreeOp::Create { id, parent, .. } => {
                if id == &self.tree.root || self.tree.nodes.contains_key(id) {
                    return Outcome::Invalid; // id collision: nothing to preserve
                }
                if !self.tree.nodes.contains_key(parent) {
                    return Outcome::Retryable; // parent not (yet) present
                }
                match self.tree.apply(&event.op) {
                    Ok(()) => {
                        self.creator.insert(*id, event.event_id.clone());
                        self.last_mutator.insert(*id, event.event_id.clone());
                        Outcome::Applied
                    }
                    // Parent exists but is a file: permanent, preserve.
                    Err(TreeError::NotAFolder(_)) => {
                        self.orphan_create(event);
                        Outcome::Applied
                    }
                    Err(TreeError::UnknownNode(_)) => Outcome::Retryable,
                    Err(_) => Outcome::Invalid, // invalid name etc.
                }
            }
            TreeOp::Rename { id, .. } => {
                if !self.tree.nodes.contains_key(id) {
                    return Outcome::Retryable; // not created yet / deleted
                }
                // Validate on a probe before any snapshot side effect:
                // a retried op must not snapshot twice.
                if let Some(outcome) = self.probe(event) {
                    return outcome;
                }
                self.snapshot_if_racing(event, *id);
                self.mutate(event, *id)
            }
            TreeOp::Move { id, new_parent } => {
                if !self.tree.nodes.contains_key(id) {
                    return Outcome::Retryable;
                }
                if !self.tree.nodes.contains_key(new_parent) {
                    return Outcome::Retryable; // destination not (yet) present
                }
                if let Some(outcome) = self.probe(event) {
                    return outcome;
                }
                self.snapshot_if_racing(event, *id);
                self.mutate(event, *id)
            }
            TreeOp::Delete { id } => {
                if !self.tree.nodes.contains_key(id) {
                    return Outcome::Retryable;
                }
                if let Some(outcome) = self.probe(event) {
                    return outcome;
                };
                // Concurrent creations under the doomed folder: children
                // the deleter never saw survive under Conflicts/.
                if let Some(known) = &event.known_children {
                    let extras: Vec<NodeId> = self
                        .tree
                        .nodes
                        .values()
                        .filter(|n| n.parent == Some(*id) && !known.contains(&n.id))
                        .map(|n| n.id)
                        .collect();
                    for extra in extras {
                        let loser = self
                            .creator
                            .get(&extra)
                            .cloned()
                            .unwrap_or_else(|| event.event_id.clone());
                        self.snapshot_subtree(extra, &loser);
                    }
                }
                // A rename/move that raced this delete: preserve the
                // loser's state before the (winning) deletion cascades.
                self.snapshot_if_racing(event, *id);
                match self.tree.apply(&event.op) {
                    Ok(()) => Outcome::Applied,
                    Err(TreeError::UnknownNode(_)) => Outcome::Retryable,
                    Err(_) => Outcome::Invalid,
                }
            }
        }
    }

    /// Dry-run the op on a throwaway clone. Returns `Some(outcome)` when
    /// the op would NOT apply (so callers skip snapshot side effects),
    /// `None` when it is safe to proceed.
    fn probe(&self, event: &TreeOpEvent) -> Option<Outcome> {
        match self.tree.clone().apply(&event.op) {
            Ok(()) => None,
            Err(TreeError::UnknownNode(_) | TreeError::CycleDetected(_)) => {
                Some(Outcome::Retryable)
            }
            Err(_) => Some(Outcome::Invalid),
        }
    }

    /// If the event claims a `base` that no longer matches the node's
    /// current state, a concurrent edit raced it: snapshot the current
    /// (loser) state into `Conflicts/` before the winner applies.
    fn snapshot_if_racing(&mut self, event: &TreeOpEvent, id: NodeId) {
        let Some(base) = &event.base else {
            return; // no claim: lenient
        };
        let Some(node) = self.tree.nodes.get(&id) else {
            return;
        };
        if node.name == base.name && node.parent == Some(base.parent) {
            return; // sequential edit built on the current state
        }
        let loser = self
            .last_mutator
            .get(&id)
            .cloned()
            .unwrap_or_else(|| event.event_id.clone());
        self.snapshot_subtree(id, &loser);
    }

    /// Apply a rename/move and record the mutator.
    fn mutate(&mut self, event: &TreeOpEvent, id: NodeId) -> Outcome {
        let outcome = match self.tree.apply(&event.op) {
            Ok(()) => Outcome::Applied,
            // Cycles are state-dependent: a later op may move the
            // obstacle out of the way.
            Err(TreeError::UnknownNode(_) | TreeError::CycleDetected(_)) => Outcome::Retryable,
            Err(_) => Outcome::Invalid,
        };
        if matches!(outcome, Outcome::Applied) {
            self.last_mutator.insert(id, event.event_id.clone());
        }
        outcome
    }

    /// Snapshot `node` and its whole subtree as copies under `Conflicts/`,
    /// attributing the copies to `losing_event_id`. Copies get
    /// deterministic ids so every device mints identical ones.
    fn snapshot_subtree(&mut self, node: NodeId, losing_event_id: &str) {
        if self.ensure_conflicts_folder().is_err() {
            return; // cannot preserve (id collision — practically impossible)
        }
        // Collect the node + descendants (via parent chains over the
        // public nodes map), deepest-last so parents are copied first.
        let mut doomed: Vec<NodeId> = self
            .tree
            .nodes
            .values()
            .filter(|n| self.is_under(n.id, node))
            .map(|n| n.id)
            .collect();
        doomed.push(node);
        doomed.sort_by_key(|id| self.depth(*id));

        let mut copy_of: BTreeMap<NodeId, NodeId> = BTreeMap::new();
        for original in doomed {
            let (kind, name, parent) = {
                let n = &self.tree.nodes[&original];
                (n.kind, n.name.clone(), n.parent)
            };
            let copy_parent = if original == node {
                CONFLICTS_ID
            } else {
                // Parents are copied before children (depth ordering),
                // so the mapping must exist — unless the copy was
                // already present and reused below.
                copy_of[&parent.expect("descendants have parents")]
            };
            let copy = conflict_copy_id(losing_event_id, original);
            // Idempotent: two conflicts can attribute to the same loser
            // (e.g. a rename race and a delete's extras both blame the
            // create event). The loser state is already preserved.
            if self.tree.nodes.contains_key(&copy) {
                copy_of.insert(original, copy);
                continue;
            }
            if self
                .tree
                .apply(&TreeOp::Create {
                    id: copy,
                    parent: copy_parent,
                    kind,
                    name,
                })
                .is_ok()
            {
                copy_of.insert(original, copy);
                self.conflicts.push(ConflictRecord {
                    losing_event_id: losing_event_id.to_owned(),
                    node: original,
                    copy,
                });
            }
        }
    }

    /// `candidate` is `ancestor` or any descendant of it.
    fn is_under(&self, candidate: NodeId, ancestor: NodeId) -> bool {
        let mut cursor = self.tree.nodes.get(&candidate);
        while let Some(node) = cursor {
            match node.parent {
                Some(parent) if parent == ancestor => return true,
                Some(parent) => cursor = self.tree.nodes.get(&parent),
                None => return false,
            }
        }
        false
    }

    fn depth(&self, id: NodeId) -> usize {
        let mut depth = 0;
        let mut cursor = self.tree.nodes.get(&id);
        while let Some(node) = cursor {
            depth += 1;
            cursor = node.parent.and_then(|p| self.tree.nodes.get(&p));
        }
        depth
    }

    fn ensure_conflicts_folder(&mut self) -> Result<bool, TreeError> {
        ensure_conflicts_folder(&mut self.tree)
    }

    /// Preserve a still-unsatisfiable create under `Conflicts/`.
    fn orphan_create(&mut self, event: &TreeOpEvent) {
        orphan_create(&mut self.tree, &mut self.conflicts, event);
    }
}

/// Replay a set of tree-op events into a tree. Deterministic and
/// arrival-order independent: events are deduplicated by event id and
/// applied in canonical `(origin_server_ts, event_id)` order (see
/// [`canonicalize`]); unsatisfiable ops are preserved or reported per the
/// conflict rules.
pub fn replay(events: &[TreeOpEvent]) -> ReplayOutput {
    let ordered = canonicalize(events);
    let mut engine = Engine {
        tree: FileTree::new(),
        conflicts: Vec::new(),
        creator: BTreeMap::new(),
        last_mutator: BTreeMap::new(),
    };
    let mut lost = Vec::new();
    let mut pending: Vec<TreeOpEvent> = Vec::new();

    for event in &ordered {
        match engine.apply_event(event) {
            Outcome::Applied => {}
            Outcome::Retryable => pending.push(event.clone()),
            Outcome::Invalid => lost.push(event.clone()),
        }
    }
    // Fixed point: retry pending ops (canonical order) until a full pass
    // applies none of them.
    if !pending.is_empty() {
        loop {
            let mut progressed = false;
            let mut still: Vec<TreeOpEvent> = Vec::new();
            for event in pending.drain(..) {
                match engine.apply_event(&event) {
                    Outcome::Applied => progressed = true,
                    Outcome::Retryable => still.push(event),
                    Outcome::Invalid => {
                        progressed = true;
                        lost.push(event);
                    }
                }
            }
            pending = still;
            if !progressed {
                break;
            }
        }
    }
    // Terminal pending ops: unsatisfiable creates are preserved under
    // Conflicts/; everything else is reported lost.
    for event in pending {
        if matches!(event.op, TreeOp::Create { .. }) {
            engine.orphan_create(&event);
        } else {
            lost.push(event);
        }
    }
    ReplayOutput {
        tree: engine.tree,
        conflicts: engine.conflicts,
        lost,
    }
}

/// The state of a node as the author of an op believed it to be at
/// authoring time: the concurrency signal used to detect races.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeBase {
    /// Believed name of the target node.
    pub name: String,
    /// Believed parent of the target node.
    pub parent: NodeId,
}

/// What travels inside the encrypted room event (`zikos.tree.op`):
/// the operation plus conflict metadata. Envelope fields (author,
/// origin server timestamp, event id) come from the room event itself.
///
/// Deserialization is lenient: unknown fields are ignored and missing
/// optional fields default to `None`, so older clients tolerate newer
/// content.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TreeOpContent {
    /// The tree mutation itself.
    pub op: TreeOp,
    /// Believed `(name, parent)` of the target at authoring time
    /// (rename/move/delete only; `None` = no claim, apply leniently).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base: Option<NodeBase>,
    /// For deletes: the child ids the author saw. Children beyond this
    /// set are concurrent creations preserved under `Conflicts/`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub known_children: Option<BTreeSet<NodeId>>,
}

/// A tree op event merged with its room-event envelope: the input to
/// replay and conflict resolution.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TreeOpEvent {
    /// The tree mutation itself.
    pub op: TreeOp,
    /// Full Matrix ID of the sender.
    pub author: String,
    /// Origin server timestamp, milliseconds since the Unix epoch.
    pub origin_server_ts: u64,
    /// Server-assigned event ID (deduplication key).
    pub event_id: String,
    /// Believed `(name, parent)` of the target at authoring time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base: Option<NodeBase>,
    /// For deletes: the child ids the author saw.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub known_children: Option<BTreeSet<NodeId>>,
}

impl TreeOpEvent {
    /// The wire content of this event (everything except the envelope).
    pub fn content(&self) -> TreeOpContent {
        TreeOpContent {
            op: self.op.clone(),
            base: self.base.clone(),
            known_children: self.known_children.clone(),
        }
    }

    /// Build an event from room-envelope fields plus wire content.
    pub fn from_parts(
        author: String,
        origin_server_ts: u64,
        event_id: String,
        content: TreeOpContent,
    ) -> Self {
        Self {
            op: content.op,
            author,
            origin_server_ts,
            event_id,
            base: content.base,
            known_children: content.known_children,
        }
    }
}

/// Deduplicate by event id (first occurrence wins — re-delivery never
/// re-applies) and sort into the canonical total order:
/// `(origin_server_ts, event_id)`. Replay over this order is what makes
/// the result independent of arrival order on every device.
pub fn canonicalize(events: &[TreeOpEvent]) -> Vec<TreeOpEvent> {
    let mut seen = std::collections::BTreeSet::new();
    let mut unique: Vec<TreeOpEvent> = events
        .iter()
        .filter(|e| seen.insert(e.event_id.clone()))
        .cloned()
        .collect();
    unique
        .sort_by(|a, b| (a.origin_server_ts, &a.event_id).cmp(&(b.origin_server_ts, &b.event_id)));
    unique
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::{NodeKind, ROOT_ID};
    use uuid::Uuid;

    fn event(base: Option<NodeBase>, known_children: Option<Vec<NodeId>>) -> TreeOpEvent {
        TreeOpEvent {
            op: TreeOp::Rename {
                id: ROOT_ID,
                name: "take-1.flac".into(),
            },
            author: "@alice:example.org".into(),
            origin_server_ts: 1_776_000_000_000,
            event_id: "$ev1".into(),
            base,
            known_children: known_children.map(|ids| ids.into_iter().collect()),
        }
    }

    #[test]
    fn uuid_v5_ids_are_deterministic() {
        // Conflict-copy ids are minted with uuid v5 so every device
        // derives identical ids from the same losing event.
        let a = Uuid::new_v5(&Uuid::NAMESPACE_OID, b"zikos.conflict:$ev1");
        let b = Uuid::new_v5(&Uuid::NAMESPACE_OID, b"zikos.conflict:$ev1");
        assert_eq!(a, b);
        let other = Uuid::new_v5(&Uuid::NAMESPACE_OID, b"zikos.conflict:$ev2");
        assert_ne!(a, other);
    }

    #[test]
    fn content_roundtrips_and_excludes_envelope_fields() {
        let ev = event(
            Some(NodeBase {
                name: "old.flac".into(),
                parent: ROOT_ID,
            }),
            None,
        );
        let json = serde_json::to_value(ev.content()).unwrap();
        // Envelope fields come from the room event, never the content.
        assert!(json.get("author").is_none());
        assert!(json.get("origin_server_ts").is_none());
        assert!(json.get("event_id").is_none());
        let back: TreeOpContent = serde_json::from_value(json).unwrap();
        assert_eq!(back, ev.content());
    }

    #[test]
    fn content_tolerates_unknown_fields_and_missing_optionals() {
        let json = serde_json::json!({
            "op": { "op": "create", "id": ROOT_ID.to_string(),
                    "parent": ROOT_ID.to_string(),
                    "kind": "folder", "name": "Takes" },
            "future_field": { "anything": true },
        });
        let content: TreeOpContent = serde_json::from_value(json).unwrap();
        assert_eq!(content.base, None);
        assert_eq!(content.known_children, None);
        assert_eq!(
            content.op,
            TreeOp::Create {
                id: ROOT_ID,
                parent: ROOT_ID,
                kind: NodeKind::Folder,
                name: "Takes".into(),
            }
        );
    }

    #[test]
    fn canonicalize_dedups_by_event_id() {
        let mut a = event(None, None);
        a.event_id = "$dup".into();
        let mut b = a.clone();
        b.op = TreeOp::Rename {
            id: ROOT_ID,
            name: "changed-later-copy.flac".into(),
        };
        let canonical = canonicalize(&[a, b]);
        assert_eq!(canonical.len(), 1, "duplicate event ids collapse");
        // The first occurrence wins; re-delivery never re-applies.
        assert_eq!(
            canonical[0].op,
            TreeOp::Rename {
                id: ROOT_ID,
                name: "take-1.flac".into(),
            }
        );
    }

    #[test]
    fn canonicalize_sorts_by_ts_then_event_id() {
        let mut late = event(None, None);
        late.origin_server_ts = 2_000;
        late.event_id = "$a".into();
        let mut early_same_ts = event(None, None);
        early_same_ts.origin_server_ts = 1_000;
        early_same_ts.event_id = "$z".into();
        let mut early_same_ts_tiebreak = early_same_ts.clone();
        early_same_ts_tiebreak.event_id = "$b".into();

        let canonical = canonicalize(&[late, early_same_ts_tiebreak, early_same_ts]);
        let ids: Vec<&str> = canonical.iter().map(|e| e.event_id.as_str()).collect();
        assert_eq!(ids, ["$b", "$z", "$a"], "order = (ts, event_id)");
    }

    /// A valid 50-op log with strictly increasing timestamps, so canonical
    /// order equals emission order.
    fn fifty_op_log() -> (Vec<TreeOpEvent>, crate::tree::FileTree) {
        use crate::tree::FileTree;
        let mut tree = FileTree::new();
        let mut events = Vec::new();
        let mut ts = 1_000u64;
        let mut folders = vec![tree.root];
        for i in 0..50 {
            let op = if i % 10 == 9 {
                // Delete the second-oldest still-alive folder occasionally.
                let victim = folders.remove(1.max(folders.len() - 1));
                TreeOp::Delete { id: victim }
            } else if i % 2 == 0 {
                let id = uuid::Uuid::new_v4();
                let parent = folders[0];
                folders.push(id);
                TreeOp::Create {
                    id,
                    parent,
                    kind: NodeKind::Folder,
                    name: format!("folder-{i}"),
                }
            } else {
                let id = uuid::Uuid::new_v4();
                let parent = folders[folders.len() - 1];
                TreeOp::Create {
                    id,
                    parent,
                    kind: NodeKind::File,
                    name: format!("file-{i}.flac"),
                }
            };
            tree.apply(&op).unwrap();
            events.push(TreeOpEvent {
                op: op.clone(),
                author: "@alice:example.org".into(),
                origin_server_ts: ts,
                event_id: format!("$ev{i}"),
                base: None,
                known_children: None,
            });
            ts += 10;
        }
        (events, tree)
    }

    #[test]
    fn replaying_a_clean_log_matches_sequential_application() {
        let (events, expected) = fifty_op_log();
        let out = replay(&events);
        assert_eq!(out.tree, expected);
        assert!(out.conflicts.is_empty());
        assert!(out.lost.is_empty());
    }

    #[test]
    fn terminal_unsatisfiable_ops_orphan_or_report() {
        // A create whose parent never appears is preserved under
        // Conflicts/ instead of vanishing (spec scenario: create vs
        // delete of parent).
        let ghost_parent = Uuid::new_v4();
        let orphan = Uuid::new_v4();
        let create = TreeOpEvent {
            op: TreeOp::Create {
                id: orphan,
                parent: ghost_parent,
                kind: NodeKind::File,
                name: "take-9.flac".into(),
            },
            author: "@alice:example.org".into(),
            origin_server_ts: 1_000,
            event_id: "$orphan".into(),
            base: None,
            known_children: None,
        };
        // A rename targeting a node that never exists anywhere is
        // reported lost (spec scenario: late operation on a deleted
        // node).
        let ghost = Uuid::new_v4();
        let rename = TreeOpEvent {
            op: TreeOp::Rename {
                id: ghost,
                name: "x.flac".into(),
            },
            author: "@alice:example.org".into(),
            origin_server_ts: 2_000,
            event_id: "$lost".into(),
            base: None,
            known_children: None,
        };

        let out = replay(&[create, rename.clone()]);
        let expected_copy = Uuid::new_v5(
            &Uuid::NAMESPACE_OID,
            format!("zikos.conflict:$orphan:{orphan}").as_bytes(),
        );
        assert_eq!(
            out.tree.find_by_path("Conflicts/take-9.flac").unwrap(),
            Some(expected_copy),
            "orphaned create is preserved under Conflicts/ with a deterministic copy id"
        );
        assert_eq!(
            out.conflicts,
            vec![ConflictRecord {
                losing_event_id: "$orphan".into(),
                node: orphan,
                copy: expected_copy,
            }]
        );
        assert_eq!(
            out.lost,
            vec![rename],
            "unsatisfiable rename is reported lost"
        );
    }

    /// An event builder for conflict tests, with increasing timestamps.
    struct EventBuilder {
        ts: u64,
        seq: u64,
    }

    impl EventBuilder {
        fn new() -> Self {
            Self { ts: 900, seq: 0 }
        }
        fn build(
            &mut self,
            author: &str,
            op: TreeOp,
            base: Option<NodeBase>,
            known_children: Option<Vec<NodeId>>,
        ) -> TreeOpEvent {
            self.seq += 1;
            self.ts += 1;
            TreeOpEvent {
                op,
                author: author.into(),
                origin_server_ts: self.ts,
                event_id: format!("$eb{}-{}", self.seq, self.ts),
                base,
                known_children: known_children.map(|ids| ids.into_iter().collect()),
            }
        }
    }

    /// A clean prefix: folder `Takes` containing file `take-1.flac`.
    fn conflict_setup() -> (Vec<TreeOpEvent>, NodeId, NodeId) {
        let takes = Uuid::new_v4();
        let file = Uuid::new_v4();
        let mut b = EventBuilder::new();
        let events = vec![
            b.build(
                "@alice:example.org",
                TreeOp::Create {
                    id: takes,
                    parent: ROOT_ID,
                    kind: NodeKind::Folder,
                    name: "Takes".into(),
                },
                None,
                None,
            ),
            b.build(
                "@alice:example.org",
                TreeOp::Create {
                    id: file,
                    parent: takes,
                    kind: NodeKind::File,
                    name: "take-1.flac".into(),
                },
                None,
                None,
            ),
        ];
        (events, takes, file)
    }

    #[test]
    fn concurrent_rename_lww_with_conflict_copy() {
        let (mut events, takes, file) = conflict_setup();
        let base = NodeBase {
            name: "take-1.flac".into(),
            parent: takes,
        };
        // Both devices saw the file as take-1.flac under Takes; both
        // rename offline. The greater (ts, event id) wins.
        let mut loser = TreeOpEvent {
            op: TreeOp::Rename {
                id: file,
                name: "vox-take.flac".into(),
            },
            author: "@bob:example.org".into(),
            origin_server_ts: 1_000,
            event_id: "$race-loser".into(),
            base: Some(base.clone()),
            known_children: None,
        };
        let mut winner = TreeOpEvent {
            op: TreeOp::Rename {
                id: file,
                name: "drum-take.flac".into(),
            },
            author: "@carol:example.org".into(),
            origin_server_ts: 1_001,
            event_id: "$race-winner".into(),
            base: Some(base),
            known_children: None,
        };
        if (loser.origin_server_ts, &loser.event_id) > (winner.origin_server_ts, &winner.event_id) {
            std::mem::swap(&mut loser, &mut winner);
        }
        let loser_event_id = loser.event_id.clone();
        let loser_name = match &loser.op {
            TreeOp::Rename { name, .. } => name.clone(),
            _ => unreachable!(),
        };
        let winner_name = match &winner.op {
            TreeOp::Rename { name, .. } => name.clone(),
            _ => unreachable!(),
        };
        events.push(loser);
        events.push(winner);

        let out = replay(&events);
        // The winner's name is the final name of the original node.
        assert_eq!(
            out.tree
                .find_by_path(&format!("Takes/{winner_name}"))
                .unwrap(),
            Some(file)
        );
        // The loser's name survives under Conflicts/ with a deterministic
        // copy id derived from the losing event.
        let expected_copy = Uuid::new_v5(
            &Uuid::NAMESPACE_OID,
            format!("zikos.conflict:{loser_event_id}:{file}").as_bytes(),
        );
        assert_eq!(
            out.tree
                .find_by_path(&format!("Conflicts/{loser_name}"))
                .unwrap(),
            Some(expected_copy)
        );
        assert_eq!(
            out.conflicts,
            vec![ConflictRecord {
                losing_event_id: loser_event_id.clone(),
                node: file,
                copy: expected_copy,
            }]
        );
        // Permutation invariance of the whole outcome.
        let mut reversed = events.clone();
        reversed.reverse();
        assert_eq!(replay(&reversed), out);
    }

    #[test]
    fn sequential_rename_produces_no_conflicts() {
        // The second rename is based on the first rename's result: its
        // base matches, so it is a clean sequential edit — no Conflicts/.
        let (mut events, takes, file) = conflict_setup();
        let mut b = EventBuilder::new();
        b.ts = 1_000;
        events.push(b.build(
            "@bob:example.org",
            TreeOp::Rename {
                id: file,
                name: "vox-take.flac".into(),
            },
            Some(NodeBase {
                name: "take-1.flac".into(),
                parent: takes,
            }),
            None,
        ));
        events.push(b.build(
            "@carol:example.org",
            TreeOp::Rename {
                id: file,
                name: "drum-take.flac".into(),
            },
            Some(NodeBase {
                name: "vox-take.flac".into(),
                parent: takes,
            }),
            None,
        ));
        let out = replay(&events);
        assert_eq!(
            out.tree.find_by_path("Takes/drum-take.flac").unwrap(),
            Some(file)
        );
        assert!(out.conflicts.is_empty());
        assert!(!out.tree.nodes.contains_key(&CONFLICTS_ID));
    }

    #[test]
    fn move_versus_delete_converges_with_subtree_preserved() {
        // Device A moves folder X (with a child inside) into P2; device B
        // concurrently deletes X based on its old location. Delete wins
        // (deletion dominates); the moved subtree is preserved under
        // Conflicts/.
        let (mut events, _takes, _file) = conflict_setup();
        let x = Uuid::new_v4();
        let child = Uuid::new_v4();
        let p2 = Uuid::new_v4();
        let mut b = EventBuilder::new();
        b.ts = 500;
        events.push(b.build(
            "@alice:example.org",
            TreeOp::Create {
                id: x,
                parent: ROOT_ID,
                kind: NodeKind::Folder,
                name: "Scraps".into(),
            },
            None,
            None,
        ));
        events.push(b.build(
            "@alice:example.org",
            TreeOp::Create {
                id: child,
                parent: x,
                kind: NodeKind::File,
                name: "child.flac".into(),
            },
            None,
            None,
        ));
        events.push(b.build(
            "@alice:example.org",
            TreeOp::Create {
                id: p2,
                parent: ROOT_ID,
                kind: NodeKind::Folder,
                name: "Archive".into(),
            },
            None,
            None,
        ));
        let move_ev = b.build(
            "@bob:example.org",
            TreeOp::Move {
                id: x,
                new_parent: p2,
            },
            Some(NodeBase {
                name: "Scraps".into(),
                parent: ROOT_ID,
            }),
            None,
        );
        let delete = b.build(
            "@carol:example.org",
            TreeOp::Delete { id: x },
            Some(NodeBase {
                name: "Scraps".into(),
                parent: ROOT_ID,
            }),
            None,
        );
        events.push(move_ev.clone());
        events.push(delete);

        let out = replay(&events);
        // Delete won: the originals are gone.
        assert!(!out.tree.nodes.contains_key(&x));
        assert!(!out.tree.nodes.contains_key(&child));
        // The moved subtree is preserved under Conflicts/ with
        // deterministic copies keyed by the losing (move) event.
        let x_copy = conflict_copy_id(&move_ev.event_id, x);
        let child_copy = conflict_copy_id(&move_ev.event_id, child);
        assert_eq!(
            out.tree
                .find_by_path("Conflicts/Scraps/child.flac")
                .unwrap(),
            Some(child_copy)
        );
        assert!(out.tree.nodes.contains_key(&x_copy));
        // A late rename targeting the deleted node is reported lost.
        let late = b.build(
            "@dora:example.org",
            TreeOp::Rename {
                id: x,
                name: "late.flac".into(),
            },
            None,
            None,
        );
        events.push(late.clone());
        let out2 = replay(&events);
        assert_eq!(out2.tree, out.tree);
        assert_eq!(out2.lost, vec![late]);
    }

    #[test]
    fn delete_preserves_concurrent_children_beyond_known_set() {
        // The deleter saw child A; child B was created concurrently.
        // B (and its subtree) survives under Conflicts/; A and the
        // folder are deleted.
        let (mut events, _takes, _file) = conflict_setup();
        let f = Uuid::new_v4();
        let a = Uuid::new_v4();
        let bs = Uuid::new_v4();
        let mut b = EventBuilder::new();
        b.ts = 500;
        events.push(b.build(
            "@alice:example.org",
            TreeOp::Create {
                id: f,
                parent: ROOT_ID,
                kind: NodeKind::Folder,
                name: "Band".into(),
            },
            None,
            None,
        ));
        events.push(b.build(
            "@alice:example.org",
            TreeOp::Create {
                id: a,
                parent: f,
                kind: NodeKind::File,
                name: "known.flac".into(),
            },
            None,
            None,
        ));
        let create_bs = b.build(
            "@carol:example.org",
            TreeOp::Create {
                id: bs,
                parent: f,
                kind: NodeKind::File,
                name: "concurrent.flac".into(),
            },
            None,
            None,
        );
        events.push(create_bs.clone());
        events.push(b.build(
            "@bob:example.org",
            TreeOp::Delete { id: f },
            None,
            Some(vec![a]),
        ));

        let out = replay(&events);
        assert!(!out.tree.nodes.contains_key(&f), "deleted folder is gone");
        assert!(!out.tree.nodes.contains_key(&a), "known child cascades");
        let bs_copy = conflict_copy_id(&create_bs.event_id, bs);
        assert_eq!(
            out.tree.find_by_path("Conflicts/concurrent.flac").unwrap(),
            Some(bs_copy),
            "the concurrent child survives under Conflicts/"
        );
        assert_eq!(
            out.conflicts,
            vec![ConflictRecord {
                losing_event_id: create_bs.event_id.clone(),
                node: bs,
                copy: bs_copy,
            }]
        );
    }

    #[test]
    fn conflicts_folder_is_lazy_and_deterministic() {
        // No conflict in a clean log → no Conflicts/ folder at all.
        let (events, _, _) = conflict_setup();
        let out = replay(&events);
        assert!(!out.tree.nodes.contains_key(&CONFLICTS_ID));
        // The same conflicting event set replayed twice (permuted) yields
        // identical trees and conflict records, including copy ids.
        let (mut racing, takes, file) = conflict_setup();
        let mut b = EventBuilder::new();
        b.ts = 1_000;
        racing.push(b.build(
            "@bob:example.org",
            TreeOp::Rename {
                id: file,
                name: "a.flac".into(),
            },
            Some(NodeBase {
                name: "take-1.flac".into(),
                parent: takes,
            }),
            None,
        ));
        racing.push(b.build(
            "@carol:example.org",
            TreeOp::Rename {
                id: file,
                name: "b.flac".into(),
            },
            Some(NodeBase {
                name: "take-1.flac".into(),
                parent: takes,
            }),
            None,
        ));
        let first = replay(&racing);
        racing.reverse();
        let second = replay(&racing);
        assert_eq!(first.tree, second.tree);
        assert_eq!(first.conflicts, second.conflicts);
        assert_eq!(first.tree.nodes[&CONFLICTS_ID].name, "Conflicts");
    }

    #[test]
    fn same_ts_inverted_create_chain_converges() {
        // Same origin-server ts; the event-id tiebreak puts the file's
        // create BEFORE its parent folder's create in canonical order.
        // The engine must pend the child and land it once the parent
        // applies (design D4.2).
        let folder = Uuid::new_v4();
        let file = Uuid::new_v4();
        let mk = |id: &str, op: TreeOp| TreeOpEvent {
            op,
            author: "@alice:example.org".into(),
            origin_server_ts: 1_000,
            event_id: id.into(),
            base: None,
            known_children: None,
        };
        let out = replay(&[
            mk(
                "$z",
                TreeOp::Create {
                    id: folder,
                    parent: ROOT_ID,
                    kind: NodeKind::Folder,
                    name: "Takes".into(),
                },
            ),
            mk(
                "$a",
                TreeOp::Create {
                    id: file,
                    parent: folder,
                    kind: NodeKind::File,
                    name: "take-1.flac".into(),
                },
            ),
        ]);
        assert_eq!(
            out.tree.find_by_path("Takes/take-1.flac").unwrap(),
            Some(file)
        );
        assert!(out.lost.is_empty());
        assert!(out.conflicts.is_empty());
    }
}

/// Property tests for the conflict rules (design D7). A small script of
/// arbitrary integers drives a generator that simulates an author's
/// model tree while emitting ops — with deliberate race injection
/// (stale bases, subset `known_children`, same-ms timestamp clusters).
#[cfg(test)]
mod prop_tests {
    use super::*;
    use crate::tree::{FileTree, ROOT_ID};
    use proptest::prelude::*;
    use std::collections::{BTreeMap, BTreeSet};

    struct Generated {
        events: Vec<TreeOpEvent>,
        /// Node ids ever inside an emitted delete's subtree at emission
        /// time (the deleter's view) — the no-silent-loss oracle.
        covered_by_delete: BTreeMap<NodeId, ()>,
    }

    /// Deterministic seeded Fisher-Yates for the permutation property.
    fn shuffled(mut v: Vec<TreeOpEvent>, seed: u64) -> Vec<TreeOpEvent> {
        let mut s = seed | 1;
        for i in (1..v.len()).rev() {
            s = s
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            let j = ((s >> 33) as usize) % (i + 1);
            v.swap(i, j);
        }
        v
    }

    struct Generator {
        tree: FileTree,
        events: Vec<TreeOpEvent>,
        covered_by_delete: BTreeMap<NodeId, ()>,
        past_bases: Vec<(NodeId, NodeBase)>,
        ts: u64,
        seq: u64,
    }

    impl Generator {
        fn new() -> Self {
            Self {
                tree: FileTree::new(),
                events: Vec::new(),
                covered_by_delete: BTreeMap::new(),
                past_bases: Vec::new(),
                ts: 100,
                seq: 0,
            }
        }

        /// Every fifth op shares the previous ts: same-ms clusters that
        /// stress the event-id tiebreak and pending logic.
        fn next_ts(&mut self, draw: u16) -> u64 {
            if draw % 5 != 0 {
                self.ts += 1 + (draw % 5) as u64;
            }
            self.ts
        }

        fn alive(&self) -> Vec<NodeId> {
            self.tree
                .nodes
                .keys()
                .copied()
                .filter(|id| *id != self.tree.root)
                .collect()
        }

        fn folders(&self) -> Vec<NodeId> {
            self.tree
                .nodes
                .iter()
                .filter(|(_, n)| n.kind == NodeKind::Folder)
                .map(|(id, _)| *id)
                .collect()
        }

        fn is_under(&self, candidate: NodeId, ancestor: NodeId) -> bool {
            let mut cursor = self.tree.nodes.get(&candidate);
            while let Some(node) = cursor {
                match node.parent {
                    Some(parent) if parent == ancestor => return true,
                    Some(parent) => cursor = self.tree.nodes.get(&parent),
                    None => return false,
                }
            }
            false
        }

        /// The base this author would truthfully attach right now, or a
        /// stale/fabricated one when injecting a race.
        fn base_for(&mut self, id: NodeId, draw: u16) -> Option<NodeBase> {
            let node = &self.tree.nodes[&id];
            let truthful = NodeBase {
                name: node.name.clone(),
                parent: node.parent.expect("non-root node"),
            };
            if draw % 4 == 0 {
                // Stale: the most recent past observation that differs,
                // else a fabricated one.
                if let Some((_, stale)) = self
                    .past_bases
                    .iter()
                    .rev()
                    .find(|(n, b)| *n == id && *b != truthful)
                {
                    return Some(stale.clone());
                }
                return Some(NodeBase {
                    name: format!("stale-{draw}"),
                    parent: self.tree.root,
                });
            }
            self.past_bases.push((id, truthful.clone()));
            Some(truthful)
        }

        /// Record the subtree a delete covers (at emission time), then
        /// emit the event — only if it applies cleanly to the model.
        fn emit(
            &mut self,
            op: TreeOp,
            base: Option<NodeBase>,
            known_children: Option<BTreeSet<NodeId>>,
            ts: u64,
            event_id: String,
        ) {
            if let TreeOp::Delete { id: target } = op {
                for other in self.tree.nodes.keys() {
                    if *other == target || self.is_under(*other, target) {
                        self.covered_by_delete.insert(*other, ());
                    }
                }
            }
            if self.tree.apply(&op).is_ok() {
                self.events.push(TreeOpEvent {
                    op,
                    author: "@gen:example.org".into(),
                    origin_server_ts: ts,
                    event_id,
                    base,
                    known_children,
                });
            }
        }

        fn run(mut self, script: &[u16]) -> Generated {
            let mut idx = 0;
            while idx + 1 < script.len() && self.events.len() < 60 {
                let action = script[idx];
                let draw = script[idx + 1];
                idx += 2;
                self.seq += 1;
                let event_id = format!("$g{}", self.seq);
                let ts = self.next_ts(draw);
                match action % 5 {
                    0 | 1 => {
                        let folders = self.folders();
                        let parent = folders[draw as usize % folders.len()];
                        let kind = if action % 2 == 0 {
                            NodeKind::Folder
                        } else {
                            NodeKind::File
                        };
                        self.emit(
                            TreeOp::Create {
                                id: Uuid::new_v4(),
                                parent,
                                kind,
                                name: format!("gen-{}", self.seq),
                            },
                            None,
                            None,
                            ts,
                            event_id,
                        );
                    }
                    2 => {
                        let alive = self.alive();
                        if alive.is_empty() {
                            continue;
                        }
                        let id = alive[draw as usize % alive.len()];
                        let base = self.base_for(id, draw);
                        self.emit(
                            TreeOp::Rename {
                                id,
                                name: format!("gen-{}", self.seq),
                            },
                            base,
                            None,
                            ts,
                            event_id,
                        );
                    }
                    3 => {
                        let alive = self.alive();
                        let folders = self.folders();
                        if alive.is_empty() || folders.len() < 2 {
                            continue;
                        }
                        let id = alive[draw as usize % alive.len()];
                        let dest = folders[(draw as usize + 1) % folders.len()];
                        if dest == id || self.is_under(dest, id) {
                            continue;
                        }
                        let base = self.base_for(id, draw);
                        self.emit(
                            TreeOp::Move {
                                id,
                                new_parent: dest,
                            },
                            base,
                            None,
                            ts,
                            event_id,
                        );
                    }
                    _ => {
                        let alive = self.alive();
                        if alive.is_empty() {
                            continue;
                        }
                        let id = alive[draw as usize % alive.len()];
                        let base = self.base_for(id, draw);
                        let children: BTreeSet<NodeId> = self
                            .tree
                            .nodes
                            .values()
                            .filter(|n| n.parent == Some(id))
                            .map(|n| n.id)
                            .collect();
                        let known = match draw % 3 {
                            0 => None,           // no claim
                            1 => Some(children), // truthful claim
                            _ => {
                                // Subset: drop one child to force a
                                // concurrent-preservation case.
                                let mut c = children;
                                c.pop_first();
                                Some(c)
                            }
                        };
                        self.emit(TreeOp::Delete { id }, base, known, ts, event_id);
                    }
                }
            }
            Generated {
                events: self.events,
                covered_by_delete: self.covered_by_delete,
            }
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(128))]

        /// Spec scenario: arrival order does not matter — any delivery
        /// permutation yields the identical replay outcome, including
        /// Conflicts/ contents and copy ids.
        #[test]
        fn permutation_invariance(script in prop::collection::vec(any::<u16>(), 8..96), seed in any::<u64>()) {
            let produced = Generator::new().run(&script);
            prop_assume!(produced.events.len() >= 2);
            let canonical = replay(&produced.events);
            let permuted = shuffled(produced.events.clone(), seed);
            let shuffled_out = replay(&permuted);
            let mut reversed = permuted.clone();
            reversed.reverse();
            let reversed_out = replay(&reversed);
            prop_assert!(shuffled_out == canonical && reversed_out == canonical);
        }

        /// Spec scenario: re-delivery is idempotent — the same events
        /// delivered twice replay exactly once.
        #[test]
        fn redelivery_idempotence(script in prop::collection::vec(any::<u16>(), 8..96)) {
            let produced = Generator::new().run(&script);
            prop_assume!(!produced.events.is_empty());
            let mut doubled = produced.events.clone();
            doubled.extend(produced.events.iter().cloned());
            prop_assert_eq!(replay(&doubled), replay(&produced.events));
        }

        /// Spec scenario: concurrent rename — the winner is exactly
        /// `max((ts, event_id))` on every device, the loser preserved
        /// under Conflicts/.
        #[test]
        fn rename_race_resolves_to_max_ts_event_id(
            ts_a in 1_000u64..1_500,
            ts_b in 1_000u64..1_500,
            eid_a in "[a-z]{3}",
            eid_b in "[a-z]{3}",
            name_a in "[a-z]{4}",
            name_b in "[a-z]{4}",
        ) {
            prop_assume!(name_a != name_b);
            let takes = Uuid::new_v4();
            let file = Uuid::new_v4();
            let mut eid_b = format!("${eid_b}");
            if eid_a == eid_b {
                eid_b.push('x');
            }
            let eid_a = format!("${eid_a}");
            let base = NodeBase { name: "take-1.flac".into(), parent: takes };
            let mut events = vec![
                TreeOpEvent {
                    op: TreeOp::Create {
                        id: takes,
                        parent: ROOT_ID,
                        kind: NodeKind::Folder,
                        name: "Takes".into(),
                    },
                    author: "@a:hs".into(),
                    origin_server_ts: 1,
                    event_id: "$setup1".into(),
                    base: None,
                    known_children: None,
                },
                TreeOpEvent {
                    op: TreeOp::Create {
                        id: file,
                        parent: takes,
                        kind: NodeKind::File,
                        name: "take-1.flac".into(),
                    },
                    author: "@a:hs".into(),
                    origin_server_ts: 2,
                    event_id: "$setup2".into(),
                    base: None,
                    known_children: None,
                },
            ];
            let mk = |name: String, ts: u64, eid: String| TreeOpEvent {
                op: TreeOp::Rename { id: file, name },
                author: "@r:hs".into(),
                origin_server_ts: ts,
                event_id: eid,
                base: Some(base.clone()),
                known_children: None,
            };
            let ev_a = mk(name_a.clone(), ts_a, eid_a.clone());
            let ev_b = mk(name_b.clone(), ts_b, eid_b.clone());
            events.push(ev_a.clone());
            events.push(ev_b.clone());

            let (winner_name, loser_name) =
                if (ts_a, &eid_a) > (ts_b, &eid_b) { (name_a, name_b) } else { (name_b, name_a) };
            let out = replay(&events);
            prop_assert_eq!(out.tree.nodes[&file].name.clone(), winner_name);
            let conflicts_names: Vec<String> = out
                .tree
                .nodes
                .values()
                .filter(|n| n.parent == Some(CONFLICTS_ID))
                .map(|n| n.name.clone())
                .collect();
            prop_assert!(
                conflicts_names.contains(&loser_name),
                "loser {loser_name} not preserved under Conflicts/ ({conflicts_names:?})"
            );
        }

        /// Design D7 property 4: every created node is in the final
        /// tree, preserved under Conflicts/, or removed by a delete that
        /// observed it per its emission-time view.
        #[test]
        fn no_silent_loss(script in prop::collection::vec(any::<u16>(), 8..96)) {
            let produced = Generator::new().run(&script);
            let out = replay(&produced.events);
            for event in &produced.events {
                let TreeOp::Create { id, .. } = &event.op else { continue };
                let preserved = out.tree.nodes.contains_key(id)
                    || out.conflicts.iter().any(|c| c.node == *id)
                    || produced.covered_by_delete.contains_key(id);
                prop_assert!(
                    preserved,
                    "created node {id} (event {}) vanished silently",
                    event.event_id
                );
            }
        }
    }
}
