//! Publish/fetch orchestration tests against a live local axum instance.

mod common;

use std::path::{Path, PathBuf};

use zikos_core::RecordingMeta;
use zikos_core::tree::{NodeKind, ROOT_ID, TreeOp};
use zikos_crypto::{CryptoError, EncryptedBlob, FileKey, decrypt_blob};
use zikos_exchange::{ExchangeError, ExchangeService};

fn meta() -> RecordingMeta {
    RecordingMeta {
        title: "take-20261022-1015".into(),
        format: zikos_core::AudioFormat::Flac,
        started_at: 1_776_000_000,
        duration_secs: 94.5,
        sample_rate_hz: 48_000,
        channels: 2,
        bpm: Some(120),
        comment: None,
    }
}

fn sample_audio() -> Vec<u8> {
    (0..50_003u32).map(|i| (i * 31 % 251) as u8).collect()
}

/// Non-temp blob files currently on disk (the server stages `.tmp` files
/// during upload; only committed blobs count).
fn blob_files(dir: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_none_or(|e| e != "tmp"))
        .collect()
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

fn payload_of(op: &TreeOp) -> zikos_core::FilePayload {
    let TreeOp::Create { file, .. } = op else {
        panic!("publish must return a create op, got {op:?}")
    };
    file.clone().expect("published creates carry a payload")
}

#[tokio::test]
async fn publish_invalid_meta_never_uploads() {
    let (client, blob_dir) = common::logged_in_client().await;
    let service = ExchangeService::new(client);
    let mut bad = meta();
    bad.title = String::new();
    let result = service
        .publish(&sample_audio(), &bad, ROOT_ID, "take-1.flac")
        .await;
    assert!(
        matches!(result, Err(ExchangeError::InvalidMeta(_))),
        "invalid meta must fail typed, got {result:?}"
    );
    assert!(
        blob_files(&blob_dir).is_empty(),
        "an invalid take must never reach the server"
    );
}

#[tokio::test]
async fn publish_uploads_ciphertext_and_returns_downloadable_op() {
    let (client, blob_dir) = common::logged_in_client().await;
    let service = ExchangeService::new(client);
    let audio = sample_audio();
    let published = service
        .publish(&audio, &meta(), ROOT_ID, "take-1.flac")
        .await
        .unwrap();

    // The returned op is a file create under the requested parent.
    let TreeOp::Create {
        id,
        parent,
        kind,
        name,
        ..
    } = published.content.op.clone()
    else {
        panic!("publish must return a create op");
    };
    assert_eq!(id, published.node_id);
    assert_eq!(parent, ROOT_ID);
    assert_eq!(kind, NodeKind::File);
    assert_eq!(name, "take-1.flac");

    // The payload references the uploaded blob and carries key + metadata.
    let payload = payload_of(&published.content.op);
    assert_eq!(payload.blob_id, published.blob_id);
    assert_eq!(payload.meta, meta());
    assert_eq!(payload.key, published.key);

    // The op is a valid tree mutation.
    let mut tree = zikos_core::FileTree::new();
    tree.apply(&published.content.op).unwrap();
    assert_eq!(tree.nodes[&published.node_id].file.as_ref(), Some(&payload));

    // Exactly one blob landed, under the returned id, as ciphertext only.
    let files = blob_files(&blob_dir);
    assert_eq!(files.len(), 1);
    assert_eq!(
        files[0].file_name().unwrap().to_str().unwrap(),
        format!("{}.bin", payload.blob_id)
    );
    let stored = std::fs::read(&files[0]).unwrap();
    assert_ne!(stored, audio, "stored bytes must not be the plaintext");
    assert!(
        !contains(&stored, &audio[100..164]),
        "stored bytes must not contain audio plaintext"
    );
    assert!(
        !contains(&stored, meta().title.as_bytes()),
        "stored bytes must not leak the file name/title"
    );

    // The frame is exactly to_wire(encrypt(key)) and decrypts to the audio.
    let blob = EncryptedBlob::from_wire(&stored).unwrap();
    assert_eq!(decrypt_blob(&payload.key, &blob).unwrap(), audio);
}

#[tokio::test]
async fn fetch_roundtrips_and_fails_closed() {
    let (client, blob_dir) = common::logged_in_client().await;
    let service = ExchangeService::new(client);
    let audio = sample_audio();
    let published = service
        .publish(&audio, &meta(), ROOT_ID, "take-1.flac")
        .await
        .unwrap();
    let payload = payload_of(&published.content.op);

    // Happy path: byte-identical audio, identical metadata.
    let fetched = service.fetch(&payload).await.unwrap();
    assert_eq!(fetched.audio, audio);
    assert_eq!(fetched.meta, meta());

    // Wrong key → typed crypto failure, never plaintext.
    let mut wrong = payload.clone();
    wrong.key = FileKey::generate();
    assert!(matches!(
        service.fetch(&wrong).await,
        Err(ExchangeError::Crypto(_))
    ));

    // Tampered at rest → typed integrity failure.
    let path = blob_dir.join(format!("{}.bin", payload.blob_id));
    let mut stored = std::fs::read(&path).unwrap();
    let last = stored.len() - 1;
    stored[last] ^= 0x01;
    std::fs::write(&path, &stored).unwrap();
    match service.fetch(&payload).await {
        Err(ExchangeError::Crypto(e)) => assert_eq!(e, CryptoError::DecryptFailed),
        other => panic!("expected typed DecryptFailed, got {other:?}"),
    }

    // Unknown blob id → typed not-found.
    let mut ghost = payload.clone();
    ghost.blob_id = "0e2d1a63-9f4e-4a2b-8c3d-4f5e6a7b8c9d".into();
    assert!(matches!(
        service.fetch(&ghost).await,
        Err(ExchangeError::NotFound(_))
    ));
}
