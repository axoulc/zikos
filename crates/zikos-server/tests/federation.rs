//! Integration tests: the real `MatrixOpenIdVerifier` against a wiremock
//! homeserver speaking the federation userinfo protocol. These pin the wire
//! contract (method, path, query parameter, response handling) that the
//! route-level fake verifier cannot see.

use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};
use zikos_server::auth::{AuthError, MatrixOpenIdVerifier, OpenIdVerifier};

fn verifier_for(server: &MockServer) -> MatrixOpenIdVerifier {
    MatrixOpenIdVerifier::with_base_url(server.uri())
}

#[tokio::test]
async fn happy_login_hits_federation_userinfo() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/_matrix/federation/v1/openid/userinfo"))
        .and(query_param("access_token", "openid-token-1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "sub": "@lisa:example.org"
        })))
        .expect(1)
        .mount(&server)
        .await;

    let verifier = verifier_for(&server);
    let user_id = verifier
        .verify("openid-token-1", "example.org")
        .await
        .expect("valid token must verify");

    assert_eq!(user_id, "@lisa:example.org");
    server.verify().await;
}

#[tokio::test]
async fn rejected_token_is_rejected() {
    let server = MockServer::start().await;
    Mock::given(path("/_matrix/federation/v1/openid/userinfo"))
        .respond_with(ResponseTemplate::new(401))
        .mount(&server)
        .await;

    let err = verifier_for(&server)
        .verify("wrong-token", "example.org")
        .await
        .expect_err("non-2xx from the homeserver must reject");
    assert!(matches!(err, AuthError::Rejected), "got {err:?}");
}

#[tokio::test]
async fn garbage_2xx_body_is_upstream_error() {
    let server = MockServer::start().await;
    Mock::given(path("/_matrix/federation/v1/openid/userinfo"))
        .respond_with(ResponseTemplate::new(200).set_body_string("not-json"))
        .mount(&server)
        .await;

    let err = verifier_for(&server)
        .verify("openid-token-1", "example.org")
        .await
        .expect_err("garbage 2xx body must be an upstream error");
    assert!(matches!(err, AuthError::Upstream), "got {err:?}");
}

#[tokio::test]
async fn sub_that_is_not_a_matrix_user_is_rejected() {
    let server = MockServer::start().await;
    Mock::given(path("/_matrix/federation/v1/openid/userinfo"))
        .respond_with(ResponseTemplate::new(200).set_body_json(
            serde_json::json!({ "sub": "lisa" }), // no '@' prefix
        ))
        .mount(&server)
        .await;

    let err = verifier_for(&server)
        .verify("openid-token-1", "example.org")
        .await
        .expect_err("a sub that is not a Matrix user id must reject");
    assert!(matches!(err, AuthError::Rejected), "got {err:?}");
}

#[tokio::test]
async fn invalid_server_name_makes_no_outbound_request() {
    let server = MockServer::start().await;
    let verifier = verifier_for(&server);
    for evil in [
        "",
        "https://matrix.example.org",
        "matrix.example.org/_matrix",
        "matrix.example.org?x=1",
        "user@matrix.example.org",
        "127.0.0.1",
    ] {
        let err = verifier
            .verify("openid-token-1", evil)
            .await
            .expect_err("{evil:?} must be rejected");
        assert!(
            matches!(err, AuthError::InvalidServerName(_)),
            "{evil:?}: got {err:?}"
        );
    }
    let received = server.received_requests().await.unwrap();
    assert!(
        received.is_empty(),
        "SSRF guard must block all outbound requests, but {} were made",
        received.len()
    );
}
