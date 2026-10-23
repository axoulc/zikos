//! End-to-end file exchange over a live local axum instance: a real
//! finalized dictaphone take is published on device A, its tree event
//! crosses to device B as JSON (what the encrypted room carries), and
//! device B replays the event, fetches the blob, and decrypts the exact
//! original audio.

mod common;

use jiff::tz::TimeZone;
use zikos_audio::{FinalizedTake, RecorderConfig, RecorderSession, SineSource};
use zikos_core::FileTree;
use zikos_core::tree::ROOT_ID;
use zikos_core::tree_sync::TreeOpContent;
use zikos_exchange::ExchangeService;
use zikos_server::routes::ServerConfig;

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

/// Record and finalize a real take with the dictaphone engine — a sine
/// source drives capture, per the audio tests (no microphone).
fn finalized_take(root: &std::path::Path) -> FinalizedTake {
    const STARTED_AT: u64 = 1_792_656_900; // 2026-10-22T08:15:00Z
    let sessions = root.join("sessions");
    let session = RecorderSession::start(
        &RecorderConfig::default(),
        Box::new(SineSource::new(48_000, 2, 24_000)),
        &sessions,
        STARTED_AT,
    )
    .unwrap();
    session.stop().unwrap().finalize(&TimeZone::UTC).unwrap()
}

#[tokio::test]
async fn finalized_take_travels_between_devices() {
    let root = tempfile::tempdir().unwrap();
    let take = finalized_take(root.path());
    let audio = std::fs::read(take.flac_path()).unwrap();
    let meta = take.meta().clone();
    assert!(!audio.is_empty(), "the fixture take must have audio");

    // --- Device A: log into the blob server and publish the take. ---
    let (base, blob_dir) = common::spawn_server(ServerConfig::default().max_blob_bytes).await;
    let publisher = ExchangeService::new(common::login_on(&base).await);
    let published = publisher
        .publish(&audio, &meta, ROOT_ID, "take-20261022-0815.flac")
        .await
        .unwrap();

    // The event crosses devices as JSON — the payload the encrypted room
    // would carry between A and B.
    let wire = serde_json::to_string(&published.content).unwrap();
    assert!(wire.contains("take-20261022-0815.flac"));

    // --- Device B: fresh client, its own session, replays and fetches. ---
    let fetcher = ExchangeService::new(common::login_on(&base).await);

    let content: TreeOpContent = serde_json::from_str(&wire).unwrap();
    let mut tree = FileTree::new();
    tree.apply(&content.op).unwrap();
    let payload = tree.nodes[&published.node_id]
        .file
        .clone()
        .expect("the replayed node carries the payload");
    assert_eq!(
        tree.find_by_path("take-20261022-0815.flac").unwrap(),
        Some(published.node_id)
    );

    let fetched = fetcher.fetch(&payload).await.unwrap();
    assert_eq!(
        fetched.audio, audio,
        "device B must receive byte-identical FLAC"
    );
    assert_eq!(&fetched.meta, take.meta());

    // --- Honest-but-curious server: on-disk bytes are ciphertext only. ---
    let stored = std::fs::read(blob_dir.join(format!("{}.bin", payload.blob_id))).unwrap();
    assert_ne!(stored, audio);
    assert!(
        !contains(&stored, &audio[..64]),
        "no audio plaintext in the stored blob"
    );
    assert!(
        !contains(&stored, meta.title.as_bytes()),
        "no file name/title in the stored blob"
    );
}
