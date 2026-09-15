use async_trait::async_trait;
use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
    routing::post,
};
use chrono::Utc;
use research_protocol::*;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    net::{IpAddr, Ipv4Addr},
    sync::Arc,
};
use tempfile::TempDir;
use tower::ServiceExt;

use crate::{
    AppState,
    acquisition::{Resolver, resolve_destination},
    budget,
    error::AppError,
    exa::{ExaContent, ExaProvider, HttpExaProvider, ProviderResponse},
};

struct UnusedExa;

#[async_trait]
impl ExaProvider for UnusedExa {
    async fn search(
        &self,
        _: &str,
        _: u32,
    ) -> Result<ProviderResponse<Vec<SearchResult>>, AppError> {
        panic!("test must not make an Exa call")
    }

    async fn contents(&self, _: &str) -> Result<ProviderResponse<Option<ExaContent>>, AppError> {
        panic!("test must not make an Exa call")
    }
}

async fn test_state(exa: Arc<dyn ExaProvider>) -> (TempDir, AppState) {
    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().join("test.db");
    let state = AppState::open(
        &format!("sqlite://{}", database.display()),
        directory.path().join("artifacts"),
        "test-token".into(),
        exa,
    )
    .await
    .unwrap();
    (directory, state)
}

#[tokio::test]
async fn zero_result_search_is_recorded_using_fake_exa_server() {
    async fn fake() -> axum::Json<Value> {
        axum::Json(json!({ "results": [], "costDollars": { "total": 0.0 } }))
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, Router::new().route("/search", post(fake)))
            .await
            .unwrap();
    });
    let provider =
        Arc::new(HttpExaProvider::new("fake-key".into(), format!("http://{address}")).unwrap());
    let (_directory, state) = test_state(provider).await;
    let pool = state.pool.clone();
    let response = crate::router(state)
        .oneshot(
            Request::post("/v1/search")
                .header("authorization", "Bearer test-token")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"query":"nothing","result_count":3}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body: SearchResponse =
        serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert_eq!(body.result_count, 0);
    let stored: (i64, String) =
        sqlx::query_as("SELECT result_count, results_json FROM searches WHERE id=?")
            .bind(body.id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(stored, (0, "[]".into()));
    server.abort();
}

#[tokio::test]
async fn provider_contents_rung_is_stored_using_fake_exa_server() {
    async fn fake() -> axum::Json<Value> {
        axum::Json(json!({
            "results": [{
                "url": "https://example.com/guide",
                "title": "Guide",
                "text": "A sufficiently complete provider extraction. ".repeat(10)
            }],
            "costDollars": { "total": 0.0 }
        }))
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, Router::new().route("/contents", post(fake)))
            .await
            .unwrap();
    });
    let provider =
        Arc::new(HttpExaProvider::new("fake-key".into(), format!("http://{address}")).unwrap());
    let (_directory, state) = test_state(provider).await;
    let pool = state.pool.clone();
    let response = crate::router(state)
        .oneshot(
            Request::post("/v1/sources")
                .header("authorization", "Bearer test-token")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"url":"https://example.com/guide","freshness_class":"stable"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body: SourceResponse =
        serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert_eq!(body.acquisition_rung, 1);
    assert!(matches!(body.content_kind, ContentKind::ProviderExtract));
    let stored: (String, String) =
        sqlx::query_as("SELECT content_kind, access_level FROM acquisitions WHERE id=?")
            .bind(body.acquisition_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(stored, ("provider_extract".into(), "full_text".into()));
    server.abort();
}

#[tokio::test]
async fn concurrent_reservations_cannot_both_spend_the_last_dollar() {
    let (_directory, state) = test_state(Arc::new(UnusedExa)).await;
    sqlx::query("INSERT INTO spend (id, operation_kind, operation_id, provider, reserved_microusd, state, idempotency_key, created_at) VALUES ('existing','test','existing','exa',14000000,'reserved','existing',?)")
        .bind(Utc::now().to_rfc3339()).execute(&state.pool).await.unwrap();
    let first = budget::reserve(&state.pool, "test", "race-a", 1_000_000);
    let second = budget::reserve(&state.pool, "test", "race-b", 1_000_000);
    let (first, second) = tokio::join!(first, second);
    assert_ne!(first.is_ok(), second.is_ok());
    let total: i64 =
        sqlx::query_scalar("SELECT SUM(reserved_microusd) FROM spend WHERE state='reserved'")
            .fetch_one(&state.pool)
            .await
            .unwrap();
    assert_eq!(total, budget::DAILY_CAP_MICRO_USD);
}

struct PrivateRedirectResolver;

#[async_trait]
impl Resolver for PrivateRedirectResolver {
    async fn resolve(&self, host: &str, _: u16) -> Result<Vec<IpAddr>, AppError> {
        assert_eq!(host, "metadata.invalid");
        Ok(vec![IpAddr::V4(Ipv4Addr::new(169, 254, 169, 254))])
    }
}

#[tokio::test]
async fn redirect_to_private_address_is_denied_before_connect() {
    // This is the redirect hop. fetch_live invokes this immediately before constructing
    // the pinned client for every hop, so no connection can be attempted first.
    let target = url::Url::parse("http://metadata.invalid/latest/meta-data").unwrap();
    let error = resolve_destination(&target, &PrivateRedirectResolver)
        .await
        .unwrap_err();
    assert!(error.to_string().starts_with("ssrf_denied:"));
}

async fn stored_extraction(state: &AppState, text: &str) -> (String, String, String) {
    let source = format!("source_{}", uuid::Uuid::new_v4());
    let acquisition = format!("acq_{}", uuid::Uuid::new_v4());
    let extraction = format!("ex_{}", uuid::Uuid::new_v4());
    let (hash, path) = state.artifacts.put(text.as_bytes()).await.unwrap();
    sqlx::query("INSERT INTO sources (id,canonical_url,origin_group,freshness_class,created_at) VALUES (?,?,'og_test','stable',?)")
        .bind(&source).bind(format!("https://example.com/{source}")).bind(Utc::now().to_rfc3339()).execute(&state.pool).await.unwrap();
    sqlx::query("INSERT INTO acquisitions (id,source_id,retrieved_at,final_url,raw_hash,raw_path,content_kind,access_level,acquisition_rung) VALUES (?,?,?,'https://example.com',?,?,'raw','full_text',2)")
        .bind(&acquisition).bind(&source).bind(Utc::now().to_rfc3339()).bind(&hash).bind(&path).execute(&state.pool).await.unwrap();
    sqlx::query("INSERT INTO extractions (id,acquisition_id,extractor,extraction_version,text_hash,text_path,created_at) VALUES (?,?,'test','1',?,?,?)")
        .bind(&extraction).bind(&acquisition).bind(hash).bind(path).bind(Utc::now().to_rfc3339()).execute(&state.pool).await.unwrap();
    (source, acquisition, extraction)
}

fn envelope(acquisition: &str, extraction: &str, body: &str) -> ReportEnvelope {
    let hash = format!("sha256:{}", hex::encode(Sha256::digest(body.as_bytes())));
    ReportEnvelope {
        schema_version: "1".into(),
        run_id: format!("run_{}", uuid::Uuid::new_v4()),
        revision: 1,
        supersedes: None,
        label: Some(Label::Reviewed),
        produced_by: ProducedBy {
            backend: "test".into(),
            backend_config: json!({}),
            instruction_version: "2026-09-14.1".into(),
            context_version: 1,
            model: None,
        },
        brief: json!({ "question": "test" }),
        assumptions: Assumptions::default(),
        body_markdown_path: "report.md".into(),
        body_hash: hash,
        claims: vec![Claim {
            id: "c1".into(),
            kind: ClaimKind::Recommendation,
            text: "Buy it".into(),
            material: true,
            evidence: vec!["ev_1".into()],
            derivation: None,
            review: ClaimReview {
                checked: false,
                by: None,
                note: None,
                outcome: None,
            },
        }],
        evidence: vec![Evidence {
            id: "ev_1".into(),
            extraction: extraction.into(),
            relation: EvidenceRelation::Supports,
            locator: json!({"kind":"section","section":"pricing"}),
            quote: "The basic plan costs ten dollars.".into(),
            normalisation: "whitespace,unicode-nfkc".into(),
            mechanical_check: None,
        }],
        assessments: vec![],
        sources: vec![ReportSource {
            acquisition: acquisition.into(),
            extraction: extraction.into(),
            content_kind: ContentKind::Raw,
            access_level: AccessLevel::FullText,
            retrieved_at: Utc::now(),
            origin_group: "og_test".into(),
        }],
        searches: vec![],
        completion: Completion {
            required_questions: vec![RequiredQuestion {
                q: "Which?".into(),
                answered: true,
                unanswerable_reason: None,
            }],
            hit_limit: None,
            what_would_change_this: "Better evidence".into(),
        },
        review: Review {
            level: ReviewLevel::Mechanical,
            records: vec![],
        },
        provider_native: None,
        artifacts: BTreeMap::new(),
    }
}

#[tokio::test]
async fn real_quote_does_not_make_wrong_conclusion_supported_and_worker_label_is_ignored() {
    let (_directory, state) = test_state(Arc::new(UnusedExa)).await;
    let (_, acquisition, extraction) =
        stored_extraction(&state, "Pricing: The basic plan costs ten dollars.").await;
    let body = "# Report\n\nBuy it.[^c1]";
    let response = crate::reports::import(
        &state.pool,
        &state.artifacts,
        ReportImportRequest {
            envelope: envelope(&acquisition, &extraction, body),
            body_markdown: body.into(),
            provider_native: None,
        },
    )
    .await
    .unwrap();
    assert_eq!(response.label, Label::Draft);
    assert!(
        response
            .reasons
            .contains(&"material_claim_not_supported:c1".into())
    );
    let (check, stored_label): (String, String) = sqlx::query_as("SELECT e.mechanical_check,r.current_computed_label FROM evidence e JOIN reports r ON r.run_id=e.run_id WHERE e.id='ev_1'")
        .fetch_one(&state.pool).await.unwrap();
    assert_eq!((check.as_str(), stored_label.as_str()), ("passed", "draft"));
}

#[tokio::test]
async fn reextraction_leaves_old_locator_valid() {
    let (_directory, state) = test_state(Arc::new(UnusedExa)).await;
    let (_, acquisition, old_extraction) =
        stored_extraction(&state, "The basic plan costs ten dollars.").await;
    let (new_hash, new_path) = state
        .artifacts
        .put(b"The page now says twenty dollars.")
        .await
        .unwrap();
    let new_extraction = format!("ex_{}", uuid::Uuid::new_v4());
    sqlx::query("INSERT INTO extractions (id,acquisition_id,extractor,extraction_version,text_hash,text_path,created_at) VALUES (?,?,'test','2',?,?,?)")
        .bind(&new_extraction).bind(&acquisition).bind(new_hash).bind(new_path).bind(Utc::now().to_rfc3339()).execute(&state.pool).await.unwrap();
    let body = "# Report\n\nOld price.[^c1]";
    let imported = crate::reports::import(
        &state.pool,
        &state.artifacts,
        ReportImportRequest {
            envelope: envelope(&acquisition, &old_extraction, body),
            body_markdown: body.into(),
            provider_native: None,
        },
    )
    .await
    .unwrap();
    assert_eq!(imported.label, Label::Draft);
    let bound: String = sqlx::query_scalar("SELECT extraction_id FROM evidence WHERE id='ev_1'")
        .fetch_one(&state.pool)
        .await
        .unwrap();
    assert_eq!(bound, old_extraction);
    assert_ne!(bound, new_extraction);
}

#[tokio::test]
async fn bearer_auth_is_required() {
    let (_directory, state) = test_state(Arc::new(UnusedExa)).await;
    let response = crate::router(state)
        .oneshot(Request::get("/v1/summary").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}
