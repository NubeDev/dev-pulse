//! Wiremock-backed coverage of [`dp_fetcher::client::Client`].
//!
//! TODO §Phase-2 requires the client wrapper to handle, in one
//! place, each of: happy, 304, 401, 403-secondary-rate, 429, 5xx.
//! This test file pins one case per branch against a wiremock
//! server so future refactors of the wrapper cannot silently
//! regress the contract reconciler/backfill depend on.

use chrono::Utc;
use dp_fetcher::client::{Client, ClientError, Fetched};
use secrecy::SecretString;
use serde_json::json;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Helper: spin up a wiremock server + client pointed at it. The
/// personal-token path is used so we don't need to also stub the
/// App→installation-token exchange in every test. The same wrapper
/// code is exercised either way (token construction is the only
/// difference).
async fn fixture() -> (MockServer, Client) {
    let server = MockServer::start().await;
    let client = Client::with_personal_token(
        SecretString::from("ghp_test_token".to_string()),
        &server.uri(),
    )
    .expect("client builds");
    (server, client)
}

#[tokio::test]
async fn happy_path_returns_body_and_etag() {
    let (server, client) = fixture().await;
    Mock::given(method("GET"))
        .and(path("/repos/o/r/pulls"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("etag", "W/\"abc123\"")
                .insert_header("x-ratelimit-remaining", "4999")
                .insert_header("x-ratelimit-reset", "9999999999")
                .set_body_json(json!([{"number": 1, "title": "first"}])),
        )
        .mount(&server)
        .await;

    let out: Fetched<serde_json::Value> = client
        .list_pull_requests("o", "r", None)
        .await
        .expect("list_pull_requests succeeds");
    match out {
        Fetched::Ok { body, etag, signal } => {
            assert_eq!(body[0]["number"], 1);
            assert_eq!(etag.as_deref(), Some("W/\"abc123\""));
            assert!(signal.is_some(), "rate-limit signal should be parsed on 200");
        }
        Fetched::NotModified { .. } => panic!("expected Ok, got NotModified"),
    }
}

#[tokio::test]
async fn not_modified_skips_body_and_returns_signal() {
    let (server, client) = fixture().await;
    Mock::given(method("GET"))
        .and(path("/repos/o/r/pulls"))
        .and(header("if-none-match", "W/\"abc123\""))
        .respond_with(
            ResponseTemplate::new(304)
                .insert_header("x-ratelimit-remaining", "4998")
                .insert_header("x-ratelimit-reset", "9999999999"),
        )
        .mount(&server)
        .await;

    let out: Fetched<serde_json::Value> = client
        .list_pull_requests("o", "r", Some("W/\"abc123\""))
        .await
        .expect("conditional get succeeds");
    assert!(
        matches!(out, Fetched::NotModified { signal: Some(_) }),
        "expected NotModified with signal, got {out:?}"
    );
}

#[tokio::test]
async fn unauthorized_returns_typed_error() {
    let (server, client) = fixture().await;
    Mock::given(method("GET"))
        .and(path("/repos/o/r/pulls"))
        .respond_with(ResponseTemplate::new(401).set_body_string("Bad credentials"))
        .mount(&server)
        .await;

    let err = client
        .list_pull_requests("o", "r", None)
        .await
        .expect_err("401 must surface as ClientError::Unauthorized");
    assert!(
        matches!(err, ClientError::Unauthorized),
        "wrong variant: {err:?}"
    );
}

#[tokio::test]
async fn secondary_rate_limit_via_403_with_resource_header() {
    let (server, client) = fixture().await;
    Mock::given(method("GET"))
        .and(path("/repos/o/r/pulls"))
        .respond_with(
            ResponseTemplate::new(403)
                .insert_header("x-ratelimit-resource", "secondary")
                .insert_header("retry-after", "30")
                .set_body_string("secondary rate limit"),
        )
        .mount(&server)
        .await;

    let before = Utc::now();
    let err = client
        .list_pull_requests("o", "r", None)
        .await
        .expect_err("403 secondary must surface as SecondaryRateLimit");
    match err {
        ClientError::SecondaryRateLimit { retry_at } => {
            let delta = (retry_at - before).num_seconds();
            assert!(
                (29..=31).contains(&delta),
                "retry-after of 30s mis-applied: delta={delta}"
            );
        }
        other => panic!("wrong variant: {other:?}"),
    }
}

#[tokio::test]
async fn raw_429_maps_to_secondary_rate_limit() {
    let (server, client) = fixture().await;
    Mock::given(method("GET"))
        .and(path("/repos/o/r/pulls"))
        .respond_with(
            ResponseTemplate::new(429)
                .insert_header("retry-after", "7")
                .set_body_string("too many requests"),
        )
        .mount(&server)
        .await;

    let err = client
        .list_pull_requests("o", "r", None)
        .await
        .expect_err("429 must surface as SecondaryRateLimit");
    assert!(
        matches!(err, ClientError::SecondaryRateLimit { .. }),
        "wrong variant: {err:?}"
    );
}

#[tokio::test]
async fn server_5xx_returns_server_error_with_body() {
    let (server, client) = fixture().await;
    Mock::given(method("GET"))
        .and(path("/repos/o/r/pulls"))
        .respond_with(ResponseTemplate::new(503).set_body_string("upstream is on fire"))
        .mount(&server)
        .await;

    let err = client
        .list_pull_requests("o", "r", None)
        .await
        .expect_err("5xx must surface as Server");
    match err {
        ClientError::Server { status, body } => {
            assert_eq!(status, 503);
            assert!(body.contains("upstream is on fire"), "body lost: {body}");
        }
        other => panic!("wrong variant: {other:?}"),
    }
}
