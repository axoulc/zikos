//! Harness smoke test: the mock homeserver must carry a client through
//! login and one sync round.

mod common;

#[tokio::test]
async fn harness_endpoints_respond() {
    let server = common::spawn().await;
    common::mount_versions(&server).await;
    common::mount_login_ok(&server, common::USER_ID).await;
    common::mount_keys(&server).await;
    common::mount_sync(&server, common::empty_sync("s1")).await;

    let dir = tempfile::tempdir().unwrap();
    let client = matrix_sdk::Client::builder()
        .homeserver_url(server.uri())
        .sqlite_store(dir.path(), Some("store-pass"))
        .build()
        .await
        .unwrap();

    client
        .matrix_auth()
        .login_username("alice", "wonderland")
        .initial_device_display_name("zikos-test")
        .await
        .unwrap();
    assert_eq!(
        client.user_id().unwrap().as_str(),
        common::USER_ID,
        "login fixture must establish the session user"
    );

    client
        .sync_once(matrix_sdk::config::SyncSettings::default())
        .await
        .expect("mock /sync must satisfy one sync round");
}
