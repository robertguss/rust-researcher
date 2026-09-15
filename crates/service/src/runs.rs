use chrono::{DateTime, Utc};
use research_protocol::{
    Classification, CreateRunRequest, Depth, Label, ReconcileRequest, RunResponse, Scope,
};
use sha2::{Digest, Sha256};
use sqlx::SqlitePool;

use crate::{budget, error::AppError};

const CONTRACT_VERSION: &str = "2026-09-14.1-unverified";
const INSTRUCTION_VERSION: &str = "2026-09-14.1";

#[derive(sqlx::FromRow)]
struct RunRow {
    id: String,
    brief_json: String,
    mode: String,
    backend: String,
    backend_contract_version: String,
    execution: String,
    blocked_reason: Option<String>,
    completeness: String,
    review: String,
    label: String,
    notification: String,
    external: String,
    current_report_id: Option<String>,
    created_at: String,
    updated_at: String,
}

pub async fn create(
    pool: &SqlitePool,
    idempotency_key: &str,
    mut request: CreateRunRequest,
) -> Result<RunResponse, AppError> {
    if idempotency_key.trim().is_empty() || idempotency_key.len() > 200 {
        return Err(AppError::validation(
            "invalid_idempotency_key",
            "Idempotency-Key must contain 1 to 200 characters",
        ));
    }
    let applied_context = crate::context::latest(pool, &request.brief.scope).await?;
    if let Some(context) = &applied_context {
        if request.brief.audience.is_none() {
            request.brief.audience = context
                .document
                .get("audience")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned);
        }
        if request.brief.locale.is_none() {
            request.brief.locale = context
                .document
                .get("locale")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned);
        }
    }
    validate_request(&mut request)?;
    let request_bytes = serde_json::to_vec(&request)?;
    let request_hash = format!("sha256:{}", hex::encode(Sha256::digest(&request_bytes)));
    let run_id = format!("run_{}", uuid::Uuid::new_v4());
    let job_id = format!("job_{}", uuid::Uuid::new_v4());
    let now = Utc::now().to_rfc3339();
    let reservation = reservation_amount(&request)?;

    let mut connection = pool.acquire().await?;
    sqlx::query("BEGIN IMMEDIATE")
        .execute(&mut *connection)
        .await?;
    let result = async {
        if let Some((stored_hash, stored_run)) =
            sqlx::query_as::<_, (String, String)>("SELECT request_hash, run_id FROM idempotency_keys WHERE key=?")
                .bind(idempotency_key)
                .fetch_optional(&mut *connection)
                .await?
        {
            if stored_hash != request_hash {
                return Err(AppError::conflict(
                    "idempotency_conflict",
                    "Idempotency-Key was already used with a different request",
                ));
            }
            return Ok(stored_run);
        }
        if let Some(amount) = reservation {
            budget::reserve_in_transaction(
                &mut connection,
                "managed_run",
                &run_id,
                amount,
                idempotency_key,
            )
            .await?;
        }
        sqlx::query(
            "INSERT INTO runs (id,brief_json,mode,backend,backend_config_json,backend_contract_version,depth,max_duration_seconds,max_cost_microusd,accept_weaker_limits,follow_up_of,context_version,instruction_version,execution,completeness,review,label,notification,external,created_at,updated_at) VALUES (?,?, 'managed',?,?,?,?,?,?,?,?,?,?,'queued','none','structural','draft','pending','none',?,?)",
        )
        .bind(&run_id)
        .bind(serde_json::to_string(&request.brief)?)
        .bind(&request.backend)
        .bind(serde_json::to_string(&request.backend_config)?)
        .bind(CONTRACT_VERSION)
        .bind(depth_name(&request.brief.depth))
        .bind(request.max_duration_seconds.map(|value| value as i64))
        .bind(reservation)
        .bind(request.accept_weaker_limits)
        .bind(&request.follow_up_of)
        .bind(applied_context.as_ref().map(|context| context.version))
        .bind(INSTRUCTION_VERSION)
        .bind(&now)
        .bind(&now)
        .execute(&mut *connection)
        .await?;
        sqlx::query(
            "INSERT INTO jobs (id,run_id,task_kind,lane,state,idempotency_key,created_at,updated_at) VALUES (?,?,'research','heavy','queued',?,?,?)",
        )
        .bind(&job_id)
        .bind(&run_id)
        .bind(format!("run:{idempotency_key}"))
        .bind(&now)
        .bind(&now)
        .execute(&mut *connection)
        .await?;
        sqlx::query("INSERT INTO idempotency_keys (key,request_hash,run_id,created_at) VALUES (?,?,?,?)")
            .bind(idempotency_key)
            .bind(request_hash)
            .bind(&run_id)
            .bind(&now)
            .execute(&mut *connection)
            .await?;
        sqlx::query("INSERT INTO events (run_id,event_type,occurred_at,payload_json) VALUES (?,'run_queued',?,'{}')")
            .bind(&run_id)
            .bind(&now)
            .execute(&mut *connection)
            .await?;
        Ok(run_id)
    }
    .await;
    match result {
        Ok(id) => {
            sqlx::query("COMMIT").execute(&mut *connection).await?;
            get(pool, &id).await
        }
        Err(error) => {
            let _ = sqlx::query("ROLLBACK").execute(&mut *connection).await;
            Err(error)
        }
    }
}

fn validate_request(request: &mut CreateRunRequest) -> Result<(), AppError> {
    if request.brief.question.trim().is_empty() {
        return Err(AppError::validation(
            "invalid_brief",
            "brief.question is required",
        ));
    }
    let classification = request
        .brief
        .classification
        .get_or_insert(match request.brief.scope {
            Scope::Personal => Classification::PersonalSensitive,
            Scope::Work => Classification::WorkConfidential,
        });
    if !matches!(request.backend.as_str(), "exa-agent" | "claude-code") {
        return Err(AppError::validation(
            "unknown_backend",
            "backend must be exa-agent or claude-code",
        ));
    }
    if request.backend == "exa-agent"
        && *classification == Classification::WorkConfidential
        && request
            .classification_override_reason
            .as_ref()
            .is_none_or(|reason| reason.trim().is_empty())
    {
        return Err(AppError::forbidden(
            "classification_not_permitted",
            "exa-agent requires a recorded per-run override for work_confidential",
        ));
    }
    if request.max_duration_seconds.is_some() && !request.accept_weaker_limits {
        return Err(AppError::validation(
            "backend_limit_not_hard",
            "candidate backends have not yet verified a hard deadline; set accept_weaker_limits",
        ));
    }
    if request.backend == "claude-code"
        && request.max_cost_usd.is_some()
        && !request.accept_weaker_limits
    {
        return Err(AppError::validation(
            "backend_limit_not_hard",
            "subscription usage has no hard per-run dollar cap; set accept_weaker_limits",
        ));
    }
    Ok(())
}

fn reservation_amount(request: &CreateRunRequest) -> Result<Option<i64>, AppError> {
    if request.backend != "exa-agent" {
        return Ok(None);
    }
    let effort = request
        .backend_config
        .get("effort")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("auto");
    let fixed = match effort {
        "minimal" => Some(12_000),
        "low" => Some(25_000),
        "medium" => Some(100_000),
        "high" => Some(500_000),
        "xhigh" => Some(1_000_000),
        "auto" => None,
        "max" => None,
        _ => {
            return Err(AppError::validation(
                "invalid_backend_config",
                "unknown Exa effort",
            ));
        }
    };
    let requested = request.max_cost_usd.as_deref().map(parse_usd).transpose()?;
    if let Some(fixed) = fixed {
        if requested.is_some_and(|cap| cap < fixed) {
            return Err(AppError::validation(
                "cost_cap_below_fixed_price",
                "max_cost_usd is below the fixed effort price",
            ));
        }
        return Ok(Some(fixed));
    }
    let default = if effort == "max" {
        20_000_000
    } else {
        5_000_000
    };
    Ok(Some(requested.unwrap_or(default)))
}

fn parse_usd(value: &str) -> Result<i64, AppError> {
    let (whole, fraction) = value.split_once('.').unwrap_or((value, ""));
    if whole.is_empty()
        || !whole.bytes().all(|byte| byte.is_ascii_digit())
        || fraction.len() > 6
        || !fraction.bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err(AppError::validation(
            "invalid_cost",
            "max_cost_usd must be a non-negative decimal with at most six fractional digits",
        ));
    }
    let whole: i64 = whole
        .parse()
        .map_err(|_| AppError::validation("invalid_cost", "max_cost_usd is too large"))?;
    let fraction: i64 = format!("{fraction:0<6}").parse().unwrap_or(0);
    whole
        .checked_mul(1_000_000)
        .and_then(|value| value.checked_add(fraction))
        .filter(|value| *value > 0)
        .ok_or_else(|| {
            AppError::validation("invalid_cost", "max_cost_usd must be greater than zero")
        })
}

pub async fn get(pool: &SqlitePool, id: &str) -> Result<RunResponse, AppError> {
    let row: RunRow = sqlx::query_as("SELECT id,brief_json,mode,backend,backend_contract_version,execution,blocked_reason,completeness,review,label,notification,external,current_report_id,created_at,updated_at FROM runs WHERE id=?")
        .bind(id).fetch_optional(pool).await?.ok_or_else(|| AppError::validation("run_not_found", "run does not exist"))?;
    row.try_into()
}

pub async fn cancel(pool: &SqlitePool, id: &str) -> Result<RunResponse, AppError> {
    let now = Utc::now().to_rfc3339();
    let changed = sqlx::query("UPDATE runs SET execution='cancelled',updated_at=? WHERE id=? AND execution IN ('queued','blocked')")
        .bind(&now).bind(id).execute(pool).await?.rows_affected();
    if changed == 0 {
        let current = get(pool, id).await?;
        if current.execution != "cancelled" {
            return Err(AppError::conflict(
                "run_not_cancellable",
                "only queued or blocked runs can be cancelled before a worker exists",
            ));
        }
    }
    sqlx::query("UPDATE jobs SET state='cancelled',updated_at=? WHERE run_id=? AND state IN ('queued','blocked','retry_wait')")
        .bind(&now).bind(id).execute(pool).await?;
    get(pool, id).await
}

pub async fn reconcile(
    pool: &SqlitePool,
    id: &str,
    request: ReconcileRequest,
) -> Result<RunResponse, AppError> {
    let now = Utc::now().to_rfc3339();
    let mut connection = pool.acquire().await?;
    sqlx::query("BEGIN IMMEDIATE")
        .execute(&mut *connection)
        .await?;
    let result = async {
        let state: Option<(String, Option<String>, Option<i64>)> = sqlx::query_as(
            "SELECT execution,blocked_reason,max_cost_microusd FROM runs WHERE id=?",
        )
        .bind(id)
        .fetch_optional(&mut *connection)
        .await?;
        let (execution, blocked_reason, max_cost) = state
            .ok_or_else(|| AppError::validation("run_not_found", "run does not exist"))?;
        if execution != "blocked" || blocked_reason.as_deref() != Some("reconcile") {
            return Err(AppError::conflict(
                "reconciliation_not_required",
                "run is not blocked for reconciliation",
            ));
        }
        let event = match request {
            ReconcileRequest::Adopt { external_task_id } => {
                if external_task_id.trim().is_empty() {
                    return Err(AppError::validation(
                        "invalid_external_task_id",
                        "external_task_id is required",
                    ));
                }
                sqlx::query("UPDATE jobs SET state='queued',external_task_id=?,updated_at=? WHERE run_id=? AND state='unknown'")
                    .bind(external_task_id).bind(&now).bind(id).execute(&mut *connection).await?;
                sqlx::query("UPDATE runs SET execution='queued',blocked_reason=NULL,external='accepted',updated_at=? WHERE id=?")
                    .bind(&now).bind(id).execute(&mut *connection).await?;
                "reconciliation_adopted"
            }
            ReconcileRequest::MarkFailed => {
                sqlx::query("UPDATE jobs SET state='failed',updated_at=? WHERE run_id=? AND state='unknown'")
                    .bind(&now).bind(id).execute(&mut *connection).await?;
                sqlx::query("UPDATE runs SET execution='failed',blocked_reason=NULL,external='terminal',updated_at=? WHERE id=?")
                    .bind(&now).bind(id).execute(&mut *connection).await?;
                "reconciliation_marked_failed"
            }
            ReconcileRequest::Resubmit { accept_charge } => {
                if !accept_charge {
                    return Err(AppError::validation(
                        "charge_acceptance_required",
                        "resubmit requires accept_charge=true",
                    ));
                }
                let amount = max_cost.ok_or_else(|| {
                    AppError::validation("missing_cost_reservation", "run has no metered cost cap")
                })?;
                let operation = format!("{id}:resubmit:{}", uuid::Uuid::new_v4());
                budget::reserve_in_transaction(
                    &mut connection,
                    "managed_run_resubmit",
                    &operation,
                    amount,
                    &operation,
                )
                .await?;
                sqlx::query("UPDATE jobs SET state='queued',external_task_id=NULL,updated_at=? WHERE run_id=? AND state='unknown'")
                    .bind(&now).bind(id).execute(&mut *connection).await?;
                sqlx::query("UPDATE runs SET execution='queued',blocked_reason=NULL,external='none',updated_at=? WHERE id=?")
                    .bind(&now).bind(id).execute(&mut *connection).await?;
                "reconciliation_resubmitted"
            }
        };
        sqlx::query("INSERT INTO events (run_id,event_type,occurred_at,payload_json) VALUES (?,?,?,'{}')")
            .bind(id).bind(event).bind(&now).execute(&mut *connection).await?;
        Ok(())
    }
    .await;
    match result {
        Ok(()) => {
            sqlx::query("COMMIT").execute(&mut *connection).await?;
        }
        Err(error) => {
            let _ = sqlx::query("ROLLBACK").execute(&mut *connection).await;
            return Err(error);
        }
    }
    get(pool, id).await
}

impl TryFrom<RunRow> for RunResponse {
    type Error = AppError;

    fn try_from(row: RunRow) -> Result<Self, Self::Error> {
        Ok(Self {
            id: row.id,
            brief: serde_json::from_str(&row.brief_json)?,
            mode: row.mode,
            backend: row.backend,
            backend_contract_version: row.backend_contract_version,
            execution: row.execution,
            blocked_reason: row.blocked_reason,
            completeness: row.completeness,
            review: row.review,
            label: match row.label.as_str() {
                "draft" => Label::Draft,
                "needs_review" => Label::NeedsReview,
                "reviewed" => Label::Reviewed,
                _ => return Err(AppError::validation("invalid_run_state", "unknown label")),
            },
            notification: row.notification,
            external: row.external,
            current_report_id: row.current_report_id,
            created_at: parse_time(&row.created_at)?,
            updated_at: parse_time(&row.updated_at)?,
        })
    }
}

fn parse_time(value: &str) -> Result<DateTime<Utc>, AppError> {
    DateTime::parse_from_rfc3339(value)
        .map(|value| value.with_timezone(&Utc))
        .map_err(|_| AppError::validation("invalid_timestamp", "stored timestamp is invalid"))
}

fn depth_name(value: &Depth) -> &'static str {
    match value {
        Depth::Lookup => "lookup",
        Depth::Standard => "standard",
        Depth::Deep => "deep",
        Depth::Extended => "extended",
    }
}
