use async_trait::async_trait;
use axum::{
    Router,
    body::{Body, to_bytes},
    extract::State,
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
use tokio::sync::Mutex;
use tower::ServiceExt;

use crate::{
    AppState,
    acquisition::{Resolver, resolve_destination},
    budget,
    error::AppError,
    exa::{AgentRun, ExaContent, ExaProvider, HttpExaProvider, ProviderResponse},
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

#[test]
fn exa_search_reservation_matches_documented_tiers() {
    assert_eq!(budget::exa_search_reservation(1), 7_000);
    assert_eq!(budget::exa_search_reservation(10), 7_000);
    assert_eq!(budget::exa_search_reservation(11), 8_000);
    assert_eq!(budget::exa_search_reservation(100), 97_000);
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
    let mut submitted = envelope(&acquisition, &extraction, body);
    submitted.assessments.push(Assessment {
        at: Utc::now(),
        by: "worker-pretending-to-be-service".into(),
        policy_version: "fake".into(),
        label: Label::Reviewed,
        reasons: vec![],
    });
    submitted.searches.push(ReportSearch {
        id: "search_imported".into(),
        backend: "external".into(),
        query: "missing product evidence".into(),
        at: Utc::now(),
        result_count: 1,
        returned_urls: vec!["https://example.com/kept".into()],
        excluded: vec![ExcludedUrl {
            url: "https://example.com/paywall".into(),
            reason: "paywalled".into(),
        }],
    });
    let response = crate::reports::import(
        &state.pool,
        &state.artifacts,
        ReportImportRequest {
            envelope: submitted,
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
    let search: (i64, String) = sqlx::query_as(
        "SELECT result_count, results_json FROM searches WHERE id='search_imported'",
    )
    .fetch_one(&state.pool)
    .await
    .unwrap();
    assert_eq!(search.0, 1);
    assert!(search.1.contains("paywalled"));
    let envelope_path: String = sqlx::query_scalar("SELECT envelope_path FROM reports WHERE id=?")
        .bind(&response.report_id)
        .fetch_one(&state.pool)
        .await
        .unwrap();
    let stored: ReportEnvelope =
        serde_json::from_slice(&state.artifacts.read(&envelope_path).await.unwrap()).unwrap();
    assert_eq!(stored.assessments.len(), 1);
    assert_eq!(stored.assessments[0].by, "service");
}

#[test]
fn model_summary_cannot_support_a_material_claim() {
    let mut report = envelope("acq", "ex", "Claim.[^c1]");
    report.review.level = ReviewLevel::MaterialClaimsReviewed;
    report.review.records.push(ReviewRecord {
        by: "worker_self_review".into(),
        checked: vec!["material_claims".into()],
        at: Utc::now(),
    });
    report.claims[0].review.checked = true;
    report.claims[0].review.outcome = Some(AssessmentOutcome::Supported);
    report.sources[0].content_kind = ContentKind::ModelSummary;
    let (label, reasons) = crate::reports::compute_label(&report);
    assert_eq!(label, Label::Draft);
    assert!(reasons.contains(&"material_claim_not_supported:c1".into()));
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

fn managed_request(question: &str) -> CreateRunRequest {
    CreateRunRequest {
        brief: ResearchBrief {
            question: question.into(),
            decision: "choose".into(),
            audience: None,
            locale: Some("US".into()),
            as_of: Utc::now(),
            required_questions: vec!["What is supported?".into()],
            constraints: Value::Null,
            exclusions: Value::Null,
            assumptions: Value::Null,
            depth: Depth::Standard,
            clarification: Clarification::Assume,
            scope: Scope::Personal,
            classification: None,
            evidence_policy: Value::Null,
            output: Value::Null,
        },
        backend: "exa-agent".into(),
        backend_config: json!({ "effort": "minimal" }),
        max_duration_seconds: None,
        max_cost_usd: None,
        accept_weaker_limits: false,
        follow_up_of: None,
        classification_override_reason: None,
    }
}

#[tokio::test]
async fn managed_run_submission_is_atomic_and_idempotent() {
    let (_directory, state) = test_state(Arc::new(UnusedExa)).await;
    let request = managed_request("A");
    let first = crate::runs::create(&state.pool, "same-key", request.clone())
        .await
        .unwrap();
    let second = crate::runs::create(&state.pool, "same-key", request)
        .await
        .unwrap();
    assert_eq!(first.id, second.id);
    assert_eq!(first.execution, "queued");
    assert_eq!(
        first.brief.classification,
        Some(Classification::PersonalSensitive)
    );
    let (runs, jobs, spend): (i64, i64, i64) = (
        sqlx::query_scalar("SELECT COUNT(*) FROM runs")
            .fetch_one(&state.pool)
            .await
            .unwrap(),
        sqlx::query_scalar("SELECT COUNT(*) FROM jobs")
            .fetch_one(&state.pool)
            .await
            .unwrap(),
        sqlx::query_scalar("SELECT SUM(reserved_microusd) FROM spend")
            .fetch_one(&state.pool)
            .await
            .unwrap(),
    );
    assert_eq!((runs, jobs, spend), (1, 1, 12_000));
    let conflict = crate::runs::create(&state.pool, "same-key", managed_request("B")).await;
    assert!(
        conflict
            .unwrap_err()
            .to_string()
            .starts_with("idempotency_conflict:")
    );
}

#[tokio::test]
async fn work_confidential_exa_run_is_rejected_before_persistence() {
    let (_directory, state) = test_state(Arc::new(UnusedExa)).await;
    let mut request = managed_request("confidential");
    request.brief.scope = Scope::Work;
    let error = crate::runs::create(&state.pool, "work-key", request)
        .await
        .unwrap_err();
    assert!(
        error
            .to_string()
            .starts_with("classification_not_permitted:")
    );
    let runs: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM runs")
        .fetch_one(&state.pool)
        .await
        .unwrap();
    let spend: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM spend")
        .fetch_one(&state.pool)
        .await
        .unwrap();
    assert_eq!((runs, spend), (0, 0));
}

#[tokio::test]
async fn stale_attempt_cannot_write_after_job_is_reclaimed() {
    let (_directory, state) = test_state(Arc::new(UnusedExa)).await;
    crate::runs::create(&state.pool, "lease-key", managed_request("lease"))
        .await
        .unwrap();
    let first = crate::jobs::claim_next(&state.pool, "heavy", "worker-a", 60)
        .await
        .unwrap()
        .unwrap();
    sqlx::query("UPDATE jobs SET lease_expires_at='2000-01-01T00:00:00Z' WHERE id=?")
        .bind(&first.id)
        .execute(&state.pool)
        .await
        .unwrap();
    assert_eq!(crate::jobs::reclaim_expired(&state.pool).await.unwrap(), 1);
    let second = crate::jobs::claim_next(&state.pool, "heavy", "worker-b", 60)
        .await
        .unwrap()
        .unwrap();
    assert!(second.attempt_epoch > first.attempt_epoch);
    let stale = crate::jobs::complete(&state.pool, &first)
        .await
        .unwrap_err();
    assert!(stale.to_string().starts_with("attempt_epoch_mismatch:"));
    crate::jobs::complete(&state.pool, &second).await.unwrap();
}

#[tokio::test]
async fn context_is_versioned_and_applied_to_new_runs() {
    let (_directory, state) = test_state(Arc::new(UnusedExa)).await;
    let first = crate::context::set(
        &state.pool,
        Scope::Personal,
        json!({ "locale": "US-PA", "audience": "Robert" }),
    )
    .await
    .unwrap();
    let second = crate::context::set(
        &state.pool,
        Scope::Personal,
        json!({ "locale": "US-NY", "audience": "Robert" }),
    )
    .await
    .unwrap();
    assert_eq!((first.version, second.version), (1, 2));
    let mut request = managed_request("context");
    request.brief.locale = None;
    request.brief.audience = None;
    let run = crate::runs::create(&state.pool, "context-key", request)
        .await
        .unwrap();
    assert_eq!(run.brief.locale.as_deref(), Some("US-NY"));
    assert_eq!(run.brief.audience.as_deref(), Some("Robert"));
    let version: i64 = sqlx::query_scalar("SELECT context_version FROM runs WHERE id=?")
        .bind(run.id)
        .fetch_one(&state.pool)
        .await
        .unwrap();
    assert_eq!(version, 2);
}

#[tokio::test]
async fn expired_paid_submission_requires_reconciliation_not_retry() {
    let (_directory, state) = test_state(Arc::new(UnusedExa)).await;
    let run = crate::runs::create(&state.pool, "unknown-key", managed_request("unknown"))
        .await
        .unwrap();
    let claim = crate::jobs::claim_next(&state.pool, "heavy", "worker-a", 60)
        .await
        .unwrap()
        .unwrap();
    crate::jobs::record_external_task(&state.pool, &claim, "provider-task")
        .await
        .unwrap();
    sqlx::query("UPDATE jobs SET lease_expires_at='2000-01-01T00:00:00Z' WHERE id=?")
        .bind(&claim.id)
        .execute(&state.pool)
        .await
        .unwrap();
    crate::jobs::reclaim_expired(&state.pool).await.unwrap();
    assert!(
        crate::jobs::claim_next(&state.pool, "heavy", "worker-b", 60)
            .await
            .unwrap()
            .is_none()
    );
    let current = crate::runs::get(&state.pool, &run.id).await.unwrap();
    assert_eq!(current.execution, "blocked");
    assert_eq!(current.blocked_reason.as_deref(), Some("reconcile"));
    assert_eq!(current.external, "unreconciled");
    let resolved = crate::runs::reconcile(&state.pool, &run.id, ReconcileRequest::MarkFailed)
        .await
        .unwrap();
    assert_eq!(resolved.execution, "failed");
    assert_eq!(resolved.blocked_reason, None);
    assert_eq!(resolved.external, "terminal");
}

#[tokio::test]
async fn exa_agent_worker_submits_polls_collects_and_reconciles() {
    #[derive(Clone)]
    struct FakeAgent {
        create_body: Arc<Mutex<Option<Value>>>,
        polls: Arc<Mutex<u32>>,
    }
    async fn create(
        State(state): State<FakeAgent>,
        axum::Json(body): axum::Json<Value>,
    ) -> axum::Json<Value> {
        *state.create_body.lock().await = Some(body);
        axum::Json(json!({
            "id": "agent_run_fake",
            "status": "running",
            "stopReason": null,
            "output": {"text": "", "structured": null, "grounding": []},
            "costDollars": {"total": 0.0},
            "usage": {"searches": 0}
        }))
    }
    async fn get(State(state): State<FakeAgent>) -> axum::Json<Value> {
        *state.polls.lock().await += 1;
        axum::Json(json!({
            "id": "agent_run_fake",
            "status": "completed",
            "stopReason": "schema_satisfied",
            "output": {
                "text": "A collected answer",
                "structured": null,
                "grounding": [{"field": "text", "citations": [{"url": "https://example.com"}]}]
            },
            "costDollars": {"total": 0.025},
            "usage": {"searches": 0}
        }))
    }
    async fn contents() -> axum::Json<Value> {
        axum::Json(json!({
            "results": [{
                "url": "https://example.com",
                "title": "Example",
                "text": "Official source content supporting a future evidence review. ".repeat(8)
            }],
            "costDollars": {"total": 0.001}
        }))
    }

    let fake = FakeAgent {
        create_body: Arc::new(Mutex::new(None)),
        polls: Arc::new(Mutex::new(0)),
    };
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn({
        let fake = fake.clone();
        async move {
            axum::serve(
                listener,
                Router::new()
                    .route("/agent/runs", post(create))
                    .route("/agent/runs/{id}", axum::routing::get(get))
                    .route("/contents", post(contents))
                    .with_state(fake),
            )
            .await
            .unwrap();
        }
    });
    let provider =
        Arc::new(HttpExaProvider::new("fake-key".into(), format!("http://{address}")).unwrap());
    let (_directory, state) = test_state(provider).await;
    let mut request = managed_request("Find the supported answer");
    request.backend_config = json!({"effort": "low"});
    let run = crate::runs::create(&state.pool, "agent-worker", request)
        .await
        .unwrap();
    assert!(
        crate::worker::process_one(&state, "test-worker")
            .await
            .unwrap()
    );

    let current = crate::runs::get(&state.pool, &run.id).await.unwrap();
    assert_eq!(current.execution, "succeeded");
    assert_eq!(current.external, "terminal");
    let stored: (String, i64, String) = sqlx::query_as(
        "SELECT status,reported_microusd,output_path FROM provider_runs WHERE run_id=?",
    )
    .bind(&run.id)
    .fetch_one(&state.pool)
    .await
    .unwrap();
    assert_eq!(stored.0, "completed");
    assert_eq!(stored.1, 25_000);
    let native: Value =
        serde_json::from_slice(&state.artifacts.read(&stored.2).await.unwrap()).unwrap();
    assert_eq!(native["usage"]["searches"], 0);
    let spend: (String, i64) =
        sqlx::query_as("SELECT state,reported_microusd FROM spend WHERE operation_id=?")
            .bind(&run.id)
            .fetch_one(&state.pool)
            .await
            .unwrap();
    assert_eq!(spend, ("reconciled".into(), 25_000));
    let linked_sources: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM run_sources WHERE run_id=?")
        .bind(&run.id)
        .fetch_one(&state.pool)
        .await
        .unwrap();
    assert_eq!(linked_sources, 1);
    let sent = fake.create_body.lock().await.clone().unwrap();
    assert_eq!(sent["query"], "Find the supported answer");
    assert_eq!(sent["effort"], "low");
    assert!(sent.get("budget").is_none());
    assert_eq!(*fake.polls.lock().await, 1);

    let response = crate::router(state)
        .oneshot(
            Request::get(format!("/v1/runs/{}/provider-result", run.id))
                .header("authorization", "Bearer test-token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let result: ProviderRunResponse =
        serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert_eq!(result.reported_cost_usd.as_deref(), Some("0.025000"));
    assert_eq!(result.output["usage"]["searches"], 0);
    server.abort();
}

struct CancelReturnsCompleted;

#[async_trait]
impl ExaProvider for CancelReturnsCompleted {
    async fn search(
        &self,
        _: &str,
        _: u32,
    ) -> Result<ProviderResponse<Vec<SearchResult>>, AppError> {
        panic!("unexpected search")
    }

    async fn contents(&self, _: &str) -> Result<ProviderResponse<Option<ExaContent>>, AppError> {
        panic!("unexpected contents")
    }

    async fn cancel_agent_run(&self, id: &str) -> Result<AgentRun, AppError> {
        assert_eq!(id, "already-completed");
        Ok(AgentRun {
            id: id.into(),
            status: "completed".into(),
            stop_reason: Some("schema_satisfied".into()),
            output: json!({"text": "The provider completed before cancellation."}),
            cost_dollars: json!({"total": 0.012}),
            extra: BTreeMap::new(),
        })
    }
}

#[tokio::test]
async fn cancellation_race_preserves_remote_completion_and_charge() {
    let (_directory, state) = test_state(Arc::new(CancelReturnsCompleted)).await;
    let run = crate::runs::create(
        &state.pool,
        "cancel-race",
        managed_request("finish quickly"),
    )
    .await
    .unwrap();
    let claim = crate::jobs::claim_next(&state.pool, "heavy", "worker-a", 60)
        .await
        .unwrap()
        .unwrap();
    crate::jobs::record_external_task(&state.pool, &claim, "already-completed")
        .await
        .unwrap();

    let response = crate::router(state.clone())
        .oneshot(
            Request::post(format!("/v1/runs/{}/cancel", run.id))
                .header("authorization", "Bearer test-token")
                .header("content-type", "application/json")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let result: RunResponse =
        serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert_eq!(result.execution, "succeeded");
    assert_eq!(result.external, "terminal");
    let spend: (String, i64) =
        sqlx::query_as("SELECT state,reported_microusd FROM spend WHERE operation_id=?")
            .bind(&run.id)
            .fetch_one(&state.pool)
            .await
            .unwrap();
    assert_eq!(spend, ("reconciled".into(), 12_000));
    let provider_status: String =
        sqlx::query_scalar("SELECT status FROM provider_runs WHERE run_id=?")
            .bind(run.id)
            .fetch_one(&state.pool)
            .await
            .unwrap();
    assert_eq!(provider_status, "completed");
}
