//! Presentation-ready folder listings over a [`FileTree`]: folders first,
//! each group name-sorted, with duration/format for files and a pending
//! marker for nodes whose latest mutation has not synced yet. Pure
//! computation — no I/O — so the UI never orders or derives anything.
//! The row types live in `crate::api::files` (codegen scope).

use crate::api::files::{NodeKindView, TreeNodeView};
use std::collections::HashSet;
use zikos_core::tree::{FileTree, NodeId, NodeKind, TreeError};

/// Build the ordered listing of `folder`'s children: folders first, then
/// files, each group sorted by name (byte order, deterministic).
pub fn folder_listing(
    tree: &FileTree,
    folder: NodeId,
    pending: &HashSet<NodeId>,
) -> Result<Vec<TreeNodeView>, TreeError> {
    // `children` rejects unknown nodes and files with the right errors.
    let children = tree.children(folder)?;
    let mut folders = Vec::new();
    let mut files = Vec::new();
    for node in children {
        let view = TreeNodeView {
            id: node.id.to_string(),
            name: node.name.clone(),
            kind: match node.kind {
                NodeKind::Folder => NodeKindView::Folder,
                NodeKind::File => NodeKindView::File,
            },
            duration_secs: node.file.as_ref().map(|f| f.meta.duration_secs),
            format: node.file.as_ref().map(|f| format_label(f.meta.format)),
            pending: pending.contains(&node.id),
            path: tree.path_of(node.id)?,
        };
        match view.kind {
            NodeKindView::Folder => folders.push(view),
            NodeKindView::File => files.push(view),
        }
    }
    // `children` is already name-sorted, so each group preserves that
    // order once partitioned.
    Ok([folders, files].concat())
}

/// Lowercase UI label for a take format.
pub(crate) fn format_label(format: zikos_core::AudioFormat) -> String {
    match format {
        zikos_core::AudioFormat::Flac => "flac",
        zikos_core::AudioFormat::Wav => "wav",
        zikos_core::AudioFormat::Opus => "opus",
    }
    .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use zikos_core::tree::{Node, ROOT_ID, TreeOp};
    use zikos_core::{AudioFormat, FilePayload, RecordingMeta};
    use zikos_crypto::FileKey;

    fn meta(title: &str, format: AudioFormat, duration_secs: f64) -> RecordingMeta {
        RecordingMeta {
            title: title.into(),
            format,
            started_at: 1_792_656_900,
            duration_secs,
            sample_rate_hz: 48_000,
            channels: 2,
            bpm: None,
            comment: None,
        }
    }

    fn payload(title: &str, format: AudioFormat, duration_secs: f64) -> FilePayload {
        FilePayload {
            blob_id: format!("blob-{title}"),
            key: FileKey::generate(),
            meta: meta(title, format, duration_secs),
        }
    }

    fn file_node(id: NodeId, parent: NodeId, name: &str, file: Option<FilePayload>) -> TreeOp {
        TreeOp::Create {
            id,
            parent,
            kind: NodeKind::File,
            name: name.into(),
            file,
        }
    }

    fn folder_node(id: NodeId, parent: NodeId, name: &str) -> TreeOp {
        TreeOp::Create {
            id,
            parent,
            kind: NodeKind::Folder,
            name: name.into(),
            file: None,
        }
    }

    fn node_id(n: u128) -> NodeId {
        uuid::Uuid::from_u128(n)
    }

    /// Fixture mirroring a real conversation tree:
    /// root ├─ "Oct 23" (folder) ├─ "Takes" (folder)
    ///                        ├─ "jam-loop.wav" (0:48 wav)
    ///                        ├─ "legacy.flac" (no payload)
    ///                        └─ "take-a.flac" (2:41 flac, pending)
    ///      └─ "Rehearsals" (folder)
    fn fixture() -> FileTree {
        let ops = vec![
            folder_node(node_id(1), ROOT_ID, "Rehearsals"),
            folder_node(node_id(2), ROOT_ID, "Oct 23"),
            folder_node(node_id(3), node_id(2), "Takes"),
            file_node(
                node_id(4),
                node_id(2),
                "jam-loop.wav",
                Some(payload("jam-loop", AudioFormat::Wav, 48.0)),
            ),
            file_node(node_id(5), node_id(2), "legacy.flac", None),
            file_node(
                node_id(6),
                node_id(2),
                "take-a.flac",
                Some(payload("take-a", AudioFormat::Flac, 161.0)),
            ),
        ];
        FileTree::from_log(&ops).unwrap()
    }

    #[test]
    fn root_listing_is_folders_first_then_files_each_name_sorted() {
        let tree = fixture();
        let listing = folder_listing(&tree, ROOT_ID, &HashSet::new()).unwrap();
        let names: Vec<&str> = listing.iter().map(|n| n.name.as_str()).collect();
        assert_eq!(names, vec!["Oct 23", "Rehearsals"]);
        assert!(listing.iter().all(|n| n.kind == NodeKindView::Folder));
        assert_eq!(listing[0].path, vec!["Oct 23"]);
    }

    #[test]
    fn folder_listing_carries_duration_format_pending_and_path() {
        let tree = fixture();
        let pending: HashSet<NodeId> = [node_id(6)].into_iter().collect();
        let listing = folder_listing(&tree, node_id(2), &pending).unwrap();
        let names: Vec<&str> = listing.iter().map(|n| n.name.as_str()).collect();
        assert_eq!(
            names,
            vec!["Takes", "jam-loop.wav", "legacy.flac", "take-a.flac"]
        );

        let jam = &listing[1];
        assert_eq!(jam.kind, NodeKindView::File);
        assert_eq!(jam.duration_secs, Some(48.0));
        assert_eq!(jam.format.as_deref(), Some("wav"));
        assert!(!jam.pending);
        assert_eq!(jam.path, vec!["Oct 23", "jam-loop.wav"]);

        let legacy = &listing[2];
        assert_eq!(legacy.duration_secs, None);
        assert_eq!(legacy.format, None);

        let take = &listing[3];
        assert!(take.pending);
        assert_eq!(take.format.as_deref(), Some("flac"));
        assert_eq!(take.duration_secs, Some(161.0));
        assert_eq!(take.path, vec!["Oct 23", "take-a.flac"]);
    }

    #[test]
    fn listing_a_file_or_unknown_node_is_an_error() {
        let tree = fixture();
        assert!(folder_listing(&tree, node_id(4), &HashSet::new()).is_err());
        assert!(folder_listing(&tree, node_id(99), &HashSet::new()).is_err());
    }

    #[test]
    fn empty_folder_lists_empty() {
        let tree = FileTree::new();
        let node = Node {
            id: node_id(1),
            parent: Some(ROOT_ID),
            kind: NodeKind::Folder,
            name: "Empty".into(),
            file: None,
        };
        let mut tree = tree;
        tree.nodes.insert(node.id, node);
        assert!(
            folder_listing(&tree, node_id(1), &HashSet::new())
                .unwrap()
                .is_empty()
        );
    }
}
