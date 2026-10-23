//! The tree browser: row types and the listing of the current
//! conversation's tree.

/// Whether a row opens as a folder or shows recording details.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeKindView {
    /// Opens the folder's listing.
    Folder,
    /// Shows the recording's details sheet.
    File,
}

/// One row of the tree browser.
#[derive(Debug, Clone, PartialEq)]
pub struct TreeNodeView {
    /// Node id (UUID string) for navigation.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Folder or file.
    pub kind: NodeKindView,
    /// Recorded duration in seconds (`None` on folders and files without
    /// a payload).
    pub duration_secs: Option<f64>,
    /// Take format as a lowercase label (`"flac"` / `"wav"`), when known.
    pub format: Option<String>,
    /// The node's latest mutation is still pending sync.
    pub pending: bool,
    /// Names from the root's child down to this node, for breadcrumbs and
    /// the details sheet (`["Oct 23", "take-….flac"]`). The unnamed root
    /// is never included.
    pub path: Vec<String>,
}

/// List one folder of the current conversation's tree. `folder_id` of
/// `None` lists the tree root. Fails when signed out (`not signed in`)
/// or when no conversation is selected (`no conversation selected`).
pub fn folder_listing(folder_id: Option<String>) -> Result<Vec<TreeNodeView>, String> {
    crate::state::with_app(|app| app.files_listing(folder_id)).map_err(|e| e.to_string())
}
