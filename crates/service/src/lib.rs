pub mod acquisition;
pub mod budget;
pub mod context;
pub mod error;
pub mod exa;
pub mod jobs;
pub mod reports;
pub mod runs;
pub mod store;
pub mod worker;

use axum::{
    Json, Router,
    extract::{Path, State},
    http::HeaderMap,
    routing::{get, post},
};
use chrono::{DateTime, Datelike, Utc};
use research_protocol::{
    AccessLevel, BackendContract, ContentKind, ContextDocument, CreateRunRequest, FreshnessClass,
    ProviderRunResponse, ReconcileRequest, ReportImportRequest, ReportImportResponse, RunResponse,
    Scope, SearchRequest, SearchResponse, SetContextRequest, SourceRequest, SourceResponse,
    SummaryResponse,
};
use sqlx::{
    SqlitePool,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions},
};
use std::{collections::BTreeMap, path::Path as FsPath, str::FromStr, sync::Arc};

use acquisition::{Resolver, SystemResolver, canonicalize, fetch_live};
use budget::EXA_CONTENTS_RESERVATION_MICRO_USD;
use error::AppError;
use exa::{ExaProvider, HttpExaProvider};
use store::ArtifactStore;

type SourceRow = (String, String, String, String, String, String, i64, String);

#[derive(Clone)]
pub struct AppState {
    pub pool: SqlitePool,
    pub artifacts: ArtifactStore,
    pub exa: Arc<dyn ExaProvider>,
    pub resolver: Arc<dyn Resolver>,
    token: Arc<str>,
}

impl AppState {
    pub async fn open(
        database_url: &str,
        artifact_root: impl AsRef<FsPath>,
        token: String,
        exa: Arc<dyn ExaProvider>,
    ) -> Result<Self, AppError> {
        let options = SqliteConnectOptions::from_str(database_url)?
            .create_if_missing(true)
            .foreign_keys(true)
            .journal_mode(SqliteJournalMode::Wal)
            .busy_timeout(std::time::Duration::from_secs(5));
        let pool = SqlitePoolOptions::new()
            .max_connections(8)
            .connect_with(options)
            .await?;
        sqlx::migrate!("./migrations").run(&pool).await?;
        Ok(Self {
            pool,
            artifacts: ArtifactStore::new(artifact_root).await?,
            exa,
            resolver: Arc::new(SystemResolver),
            token: token.into(),
        })
    }

    pub fn with_resolver(mut self, resolver: Arc<dyn Resolver>) -> Self {
        self.resolver = resolver;
        self
    }
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/v1/search", post(search))
        .route("/v1/sources", post(source))
        .route("/v1/sources/{id}", get(get_source))
        .route("/v1/reports/import", post(import_report))
        .route("/v1/runs", post(create_run))
        .route("/v1/runs/{id}", get(get_run))
        .route("/v1/runs/{id}/provider-result", get(get_provider_result))
        .route("/v1/runs/{id}/cancel", post(cancel_run))
        .route("/v1/runs/{id}/reconcile", post(reconcile_run))
        .route("/v1/context/{scope}", get(get_context).put(set_context))
        .route("/v1/backends", get(backends))
        .route("/v1/summary", get(summary))
        .with_state(state)
}

fn parse_scope(value: &str) -> Result<Scope, AppError> {
    match value {
        "personal" => Ok(Scope::Personal),
        "work" => Ok(Scope::Work),
        _ => Err(AppError::validation(
            "invalid_scope",
            "scope must be personal or work",
        )),
    }
}

async fn get_context(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(scope): Path<String>,
) -> Result<Json<ContextDocument>, AppError> {
    authenticate(&headers, &state)?;
    context::latest(&state.pool, &parse_scope(&scope)?)
        .await?
        .map(Json)
        .ok_or_else(|| AppError::validation("context_not_found", "context profile does not exist"))
}

async fn set_context(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(scope): Path<String>,
    Json(request): Json<SetContextRequest>,
) -> Result<Json<ContextDocument>, AppError> {
    authenticate(&headers, &state)?;
    Ok(Json(
        context::set(&state.pool, parse_scope(&scope)?, request.document).await?,
    ))
}

async fn create_run(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<CreateRunRequest>,
) -> Result<Json<RunResponse>, AppError> {
    authenticate(&headers, &state)?;
    let key = headers
        .get("idempotency-key")
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| {
            AppError::validation("missing_idempotency_key", "Idempotency-Key is required")
        })?;
    Ok(Json(runs::create(&state.pool, key, request).await?))
}

async fn get_run(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<RunResponse>, AppError> {
    authenticate(&headers, &state)?;
    Ok(Json(runs::get(&state.pool, &id).await?))
}

async fn get_provider_result(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<ProviderRunResponse>, AppError> {
    authenticate(&headers, &state)?;
    let row: (String, String, String, Option<String>, Option<i64>, String, String) =
        sqlx::query_as("SELECT provider,external_task_id,status,stop_reason,reported_microusd,collected_at,output_path FROM provider_runs WHERE run_id=? ORDER BY collected_at DESC LIMIT 1")
            .bind(&id)
            .fetch_optional(&state.pool)
            .await?
            .ok_or_else(|| AppError::validation("provider_result_not_found", "run has no collected provider result"))?;
    let output = serde_json::from_slice(&state.artifacts.read(&row.6).await?)?;
    Ok(Json(ProviderRunResponse {
        run_id: id,
        provider: row.0,
        external_task_id: row.1,
        status: row.2,
        stop_reason: row.3,
        reported_cost_usd: row.4.map(usd),
        collected_at: DateTime::parse_from_rfc3339(&row.5)
            .map_err(|_| AppError::validation("invalid_timestamp", "stored timestamp is invalid"))?
            .with_timezone(&Utc),
        output,
    }))
}

async fn cancel_run(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<RunResponse>, AppError> {
    authenticate(&headers, &state)?;
    let active: Option<(String, String)> = sqlx::query_as(
        "SELECT j.id,j.external_task_id FROM jobs j JOIN runs r ON r.id=j.run_id WHERE r.id=? AND r.execution='running' AND j.state='running' AND j.external_task_id IS NOT NULL",
    )
    .bind(&id)
    .fetch_optional(&state.pool)
    .await?;
    if let Some((job_id, external_id)) = active {
        let provider_run = state.exa.cancel_agent_run(&external_id).await?;
        worker::record_control_result(&state, &id, &job_id, &provider_run).await?;
        return Ok(Json(runs::get(&state.pool, &id).await?));
    }
    Ok(Json(runs::cancel(&state.pool, &id).await?))
}

async fn reconcile_run(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(request): Json<ReconcileRequest>,
) -> Result<Json<RunResponse>, AppError> {
    authenticate(&headers, &state)?;
    Ok(Json(runs::reconcile(&state.pool, &id, request).await?))
}

fn authenticate(headers: &HeaderMap, state: &AppState) -> Result<(), AppError> {
    let supplied = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok());
    if supplied != Some(&format!("Bearer {}", state.token)) {
        return Err(AppError::Client {
            status: axum::http::StatusCode::UNAUTHORIZED,
            code: "unauthorized",
            message: "valid bearer token required".into(),
            retryable: false,
        });
    }
    Ok(())
}

async fn search(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<SearchRequest>,
) -> Result<Json<SearchResponse>, AppError> {
    authenticate(&headers, &state)?;
    if request.query.trim().is_empty() || !(1..=100).contains(&request.result_count) {
        return Err(AppError::validation(
            "invalid_search",
            "query must be non-empty and result_count must be 1..=100",
        ));
    }
    let id = format!("search_{}", uuid::Uuid::new_v4());
    let reservation = budget::reserve(
        &state.pool,
        "search",
        &id,
        budget::exa_search_reservation(request.result_count),
    )
    .await?;
    let provider = match state.exa.search(&request.query, request.result_count).await {
        Ok(value) => value,
        Err(error) => {
            budget::release(&state.pool, &reservation).await?;
            return Err(error);
        }
    };
    budget::reconcile(&state.pool, &reservation, provider.reported_cost_microusd).await?;
    let response = SearchResponse {
        id,
        backend: "exa-search".into(),
        query: request.query,
        at: Utc::now(),
        result_count: provider.value.len() as u32,
        results: provider.value,
    };
    sqlx::query("INSERT INTO searches (id, backend, query, searched_at, result_count, results_json) VALUES (?, ?, ?, ?, ?, ?)")
        .bind(&response.id)
        .bind(&response.backend)
        .bind(&response.query)
        .bind(response.at.to_rfc3339())
        .bind(response.result_count)
        .bind(serde_json::to_string(&response.results)?)
        .execute(&state.pool)
        .await?;
    Ok(Json(response))
}

async fn source(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<SourceRequest>,
) -> Result<Json<SourceResponse>, AppError> {
    authenticate(&headers, &state)?;
    let initial_url = canonicalize(&request.url)?;
    let operation_id = format!("source_{}", uuid::Uuid::new_v4());
    let reservation = budget::reserve(
        &state.pool,
        "source_contents",
        &operation_id,
        EXA_CONTENTS_RESERVATION_MICRO_USD,
    )
    .await?;
    let provider = match state.exa.contents(&initial_url).await {
        Ok(value) => {
            budget::reconcile(&state.pool, &reservation, value.reported_cost_microusd).await?;
            value.value
        }
        Err(_) => {
            budget::release(&state.pool, &reservation).await?;
            None
        }
    };
    let force_live =
        request.require_live || matches!(request.freshness_class, FreshnessClass::Volatile);
    if let Some(content) = provider {
        let sufficient = content.text.len() >= 200;
        let response = persist_source(
            &state,
            &initial_url,
            &initial_url,
            content.title,
            request.freshness_class.clone(),
            content.text.as_bytes(),
            &content.text,
            ContentKind::ProviderExtract,
            if sufficient {
                AccessLevel::FullText
            } else {
                AccessLevel::PartialText
            },
            1,
            "exa-contents",
        )
        .await?;
        if !force_live && sufficient {
            return Ok(Json(response));
        }
    }
    let (final_url, raw, text) = fetch_live(&initial_url, state.resolver.clone()).await?;
    if text.is_empty() {
        return Err(AppError::validation(
            "empty_extraction",
            "live page contained no readable text",
        ));
    }
    let final_url = canonicalize(final_url.as_str())?;
    let response = persist_source(
        &state,
        &final_url,
        &final_url,
        None,
        request.freshness_class,
        &raw,
        &text,
        ContentKind::Raw,
        AccessLevel::FullText,
        2,
        "readability-html-v1",
    )
    .await?;
    Ok(Json(response))
}

#[allow(clippy::too_many_arguments)]
async fn persist_source(
    state: &AppState,
    canonical_url: &str,
    final_url: &str,
    title: Option<String>,
    freshness: FreshnessClass,
    raw: &[u8],
    text: &str,
    content_kind: ContentKind,
    access_level: AccessLevel,
    rung: u8,
    extractor: &str,
) -> Result<SourceResponse, AppError> {
    let source_id = format!("source_{}", uuid::Uuid::new_v4());
    let origin_group = format!("og_{}", uuid::Uuid::new_v4());
    sqlx::query("INSERT OR IGNORE INTO sources (id, canonical_url, title, origin_group, freshness_class, created_at) VALUES (?, ?, ?, ?, ?, ?)")
        .bind(&source_id).bind(canonical_url).bind(title).bind(origin_group)
        .bind(freshness_name(&freshness)).bind(Utc::now().to_rfc3339()).execute(&state.pool).await?;
    let source_id: String = sqlx::query_scalar("SELECT id FROM sources WHERE canonical_url = ?")
        .bind(canonical_url)
        .fetch_one(&state.pool)
        .await?;
    let (raw_hash, raw_path) = state.artifacts.put(raw).await?;
    let (text_hash, text_path) = state.artifacts.put(text.as_bytes()).await?;
    let acquisition_id = format!("acq_{}", uuid::Uuid::new_v4());
    let extraction_id = format!("ex_{}", uuid::Uuid::new_v4());
    let mut tx = state.pool.begin().await?;
    sqlx::query("INSERT INTO acquisitions (id, source_id, retrieved_at, final_url, raw_hash, raw_path, content_kind, access_level, acquisition_rung) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)")
        .bind(&acquisition_id).bind(&source_id).bind(Utc::now().to_rfc3339()).bind(final_url)
        .bind(raw_hash).bind(raw_path).bind(content_kind_name(&content_kind)).bind(access_level_name(&access_level)).bind(rung)
        .execute(&mut *tx).await?;
    sqlx::query("INSERT INTO extractions (id, acquisition_id, extractor, extraction_version, text_hash, text_path, created_at) VALUES (?, ?, ?, '1', ?, ?, ?)")
        .bind(&extraction_id).bind(&acquisition_id).bind(extractor).bind(text_hash).bind(text_path)
        .bind(Utc::now().to_rfc3339()).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(SourceResponse {
        source_id,
        acquisition_id,
        extraction_id,
        canonical_url: canonical_url.into(),
        final_url: final_url.into(),
        content_kind,
        access_level,
        acquisition_rung: rung,
        text: text.into(),
    })
}

async fn get_source(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<SourceResponse>, AppError> {
    authenticate(&headers, &state)?;
    let row: Option<SourceRow> = sqlx::query_as(
        "SELECT s.canonical_url, a.id, e.id, a.final_url, a.content_kind, a.access_level, a.acquisition_rung, e.text_path FROM sources s JOIN acquisitions a ON a.source_id=s.id JOIN extractions e ON e.acquisition_id=a.id WHERE s.id=? ORDER BY a.retrieved_at DESC LIMIT 1",
    ).bind(&id).fetch_optional(&state.pool).await?;
    let (
        canonical_url,
        acquisition_id,
        extraction_id,
        final_url,
        content_kind,
        access_level,
        rung,
        text_path,
    ) = row.ok_or_else(|| AppError::validation("source_not_found", "source does not exist"))?;
    let text = String::from_utf8(state.artifacts.read(&text_path).await?)
        .map_err(|_| AppError::validation("invalid_extraction", "extraction is not UTF-8"))?;
    Ok(Json(SourceResponse {
        source_id: id,
        acquisition_id,
        extraction_id,
        canonical_url,
        final_url,
        content_kind: parse_content_kind(&content_kind)?,
        access_level: parse_access_level(&access_level)?,
        acquisition_rung: rung as u8,
        text,
    }))
}

async fn import_report(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<ReportImportRequest>,
) -> Result<Json<ReportImportResponse>, AppError> {
    authenticate(&headers, &state)?;
    Ok(Json(
        reports::import(&state.pool, &state.artifacts, request).await?,
    ))
}

async fn backends(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<BackendContract>>, AppError> {
    authenticate(&headers, &state)?;
    let capabilities = BTreeMap::from([
        ("deadline".into(), "best_effort".into()),
        ("cost_cap".into(), "hard".into()),
        ("cancellation".into(), "unverified".into()),
        ("cost_reporting".into(), "per_run".into()),
        ("citations".into(), "urls_only".into()),
        ("mid_run_clarification".into(), "unsupported".into()),
        ("resume".into(), "none".into()),
        ("structured_output".into(), "schema".into()),
        ("partial_output".into(), "unverified".into()),
        ("result_retention".into(), "unverified".into()),
        ("submission_idempotency".into(), "none".into()),
        ("privacy_mode".into(), "unverified".into()),
    ]);
    Ok(Json(vec![BackendContract {
        name: "exa-agent".into(),
        adapter_version: "2026-09-15.1".into(),
        capabilities,
    }]))
}

async fn summary(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<SummaryResponse>, AppError> {
    authenticate(&headers, &state)?;
    let now = Utc::now();
    let day = now
        .date_naive()
        .and_hms_opt(0, 0, 0)
        .expect("valid")
        .and_utc()
        .to_rfc3339();
    let month = now
        .date_naive()
        .with_day(1)
        .expect("valid")
        .and_hms_opt(0, 0, 0)
        .expect("valid")
        .and_utc()
        .to_rfc3339();
    let today: i64 = sqlx::query_scalar("SELECT COALESCE(SUM(reserved_microusd),0) FROM spend WHERE state IN ('reserved','reconciled','unknown') AND created_at>=?").bind(day).fetch_one(&state.pool).await?;
    let monthly: i64 = sqlx::query_scalar("SELECT COALESCE(SUM(reserved_microusd),0) FROM spend WHERE state IN ('reserved','reconciled','unknown') AND created_at>=?").bind(month).fetch_one(&state.pool).await?;
    Ok(Json(SummaryResponse {
        spend_today_usd: usd(today),
        daily_cap_usd: usd(budget::DAILY_CAP_MICRO_USD),
        spend_month_usd: usd(monthly),
        monthly_cap_usd: usd(budget::MONTHLY_CAP_MICRO_USD),
        unknown_spend_count: count(&state.pool, "spend", Some("state='unknown'")).await?,
        reports: count(&state.pool, "reports", None).await?,
        sources: count(&state.pool, "sources", None).await?,
        searches: count(&state.pool, "searches", None).await?,
    }))
}

async fn count(pool: &SqlitePool, table: &str, condition: Option<&str>) -> Result<u64, AppError> {
    let sql = format!(
        "SELECT COUNT(*) FROM {table}{}",
        condition
            .map(|value| format!(" WHERE {value}"))
            .unwrap_or_default()
    );
    Ok(sqlx::query_scalar::<_, i64>(&sql).fetch_one(pool).await? as u64)
}

fn usd(value: i64) -> String {
    format!("{:.6}", value as f64 / 1_000_000.0)
}
fn freshness_name(value: &FreshnessClass) -> &'static str {
    match value {
        FreshnessClass::Volatile => "volatile",
        FreshnessClass::Current => "current",
        FreshnessClass::Stable => "stable",
        FreshnessClass::Immutable => "immutable",
    }
}
fn content_kind_name(value: &ContentKind) -> &'static str {
    match value {
        ContentKind::Raw => "raw",
        ContentKind::ProviderExtract => "provider_extract",
        ContentKind::ModelSummary => "model_summary",
    }
}
fn access_level_name(value: &AccessLevel) -> &'static str {
    match value {
        AccessLevel::MetadataOnly => "metadata_only",
        AccessLevel::Snippet => "snippet",
        AccessLevel::PartialText => "partial_text",
        AccessLevel::FullText => "full_text",
    }
}
fn parse_content_kind(value: &str) -> Result<ContentKind, AppError> {
    match value {
        "raw" => Ok(ContentKind::Raw),
        "provider_extract" => Ok(ContentKind::ProviderExtract),
        "model_summary" => Ok(ContentKind::ModelSummary),
        _ => Err(AppError::validation("invalid_content_kind", value)),
    }
}
fn parse_access_level(value: &str) -> Result<AccessLevel, AppError> {
    match value {
        "metadata_only" => Ok(AccessLevel::MetadataOnly),
        "snippet" => Ok(AccessLevel::Snippet),
        "partial_text" => Ok(AccessLevel::PartialText),
        "full_text" => Ok(AccessLevel::FullText),
        _ => Err(AppError::validation("invalid_access_level", value)),
    }
}

pub fn live_exa_from_env() -> Result<Arc<dyn ExaProvider>, AppError> {
    let api_key = std::env::var("EXA_API_KEY")
        .map_err(|_| AppError::validation("missing_exa_api_key", "set EXA_API_KEY"))?;
    let base = std::env::var("EXA_BASE_URL").unwrap_or_else(|_| "https://api.exa.ai".into());
    Ok(Arc::new(HttpExaProvider::new(api_key, base)?))
}

#[cfg(test)]
mod tests;
