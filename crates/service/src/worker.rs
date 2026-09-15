use std::time::Duration;

use chrono::Utc;
use serde_json::{Value, json};

use crate::{AppState, budget, error::AppError, exa::AgentRun, jobs};

const LEASE_SECONDS: i64 = 60;
const POLL_INTERVAL: Duration = Duration::from_secs(2);

pub async fn process_one(state: &AppState, owner: &str) -> Result<bool, AppError> {
    let Some(claim) = jobs::claim_next(&state.pool, "heavy", owner, LEASE_SECONDS).await? else {
        return Ok(false);
    };
    if let Err(error) = execute_exa(state, &claim, owner).await {
        jobs::fail(&state.pool, &claim, &error.to_string()).await?;
        return Err(error);
    }
    Ok(true)
}

pub async fn record_control_result(
    state: &AppState,
    run_id: &str,
    job_id: &str,
    provider_run: &AgentRun,
) -> Result<(), AppError> {
    let bytes = serde_json::to_vec(provider_run)?;
    let (hash, path) = state.artifacts.put(&bytes).await?;
    let cost = provider_run.reported_cost_microusd();
    let reservation_id: String = sqlx::query_scalar(
        "SELECT id FROM spend WHERE operation_id=? AND state='reserved' ORDER BY created_at DESC LIMIT 1",
    )
    .bind(run_id)
    .fetch_one(&state.pool)
    .await?;
    budget::reconcile(
        &state.pool,
        &budget::Reservation { id: reservation_id },
        cost,
    )
    .await?;
    let now = Utc::now().to_rfc3339();
    let execution = match provider_run.status.as_str() {
        "completed" => "succeeded",
        "cancelled" => "cancelled",
        _ => "failed",
    };
    let job_state = match execution {
        "succeeded" => "succeeded",
        "cancelled" => "cancelled",
        _ => "failed",
    };
    let mut tx = state.pool.begin().await?;
    sqlx::query("INSERT INTO provider_runs (id,run_id,job_id,provider,external_task_id,status,stop_reason,output_hash,output_path,reported_microusd,collected_at) VALUES (?,?,?,?,?,?,?,?,?,?,?) ON CONFLICT(provider,external_task_id) DO NOTHING")
        .bind(format!("provider_run_{}", uuid::Uuid::new_v4()))
        .bind(run_id)
        .bind(job_id)
        .bind("exa-agent")
        .bind(&provider_run.id)
        .bind(&provider_run.status)
        .bind(&provider_run.stop_reason)
        .bind(hash)
        .bind(path)
        .bind(cost)
        .bind(&now)
        .execute(&mut *tx)
        .await?;
    sqlx::query("UPDATE jobs SET state=?,lease_owner=NULL,lease_expires_at=NULL,updated_at=? WHERE id=? AND state='running'")
        .bind(job_state)
        .bind(&now)
        .bind(job_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("UPDATE runs SET execution=?,external='terminal',updated_at=? WHERE id=? AND execution='running'")
        .bind(execution)
        .bind(&now)
        .bind(run_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("INSERT INTO events (run_id,event_type,occurred_at,payload_json) VALUES (?,'external_cancellation_result',?,?)")
        .bind(run_id)
        .bind(&now)
        .bind(json!({"provider_status": provider_run.status}).to_string())
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}

async fn execute_exa(
    state: &AppState,
    claim: &jobs::JobClaim,
    owner: &str,
) -> Result<(), AppError> {
    let (brief_json, backend, backend_config, max_cost, external_id): (
        String,
        String,
        String,
        Option<i64>,
        Option<String>,
    ) = sqlx::query_as(
        "SELECT r.brief_json,r.backend,r.backend_config_json,r.max_cost_microusd,j.external_task_id FROM runs r JOIN jobs j ON j.run_id=r.id WHERE j.id=?",
    )
    .bind(&claim.id)
    .fetch_one(&state.pool)
    .await?;
    if backend != "exa-agent" {
        return Err(AppError::validation(
            "worker_backend_not_implemented",
            "worker only supports exa-agent",
        ));
    }

    let brief: Value = serde_json::from_str(&brief_json)?;
    let config: Value = serde_json::from_str(&backend_config)?;
    let effort = config
        .get("effort")
        .and_then(Value::as_str)
        .unwrap_or("auto");
    let mut request = json!({
        "query": brief.get("question").and_then(Value::as_str).unwrap_or_default(),
        "effort": effort,
        "metadata": {"research_run_id": claim.run_id}
    });
    if let Some(value) = config.get("systemPrompt") {
        request["systemPrompt"] = value.clone();
    }
    if let Some(value) = config.get("outputSchema") {
        request["outputSchema"] = value.clone();
    }
    if matches!(effort, "auto" | "max") {
        request["budget"] = json!({
            "maxCostDollars": max_cost.unwrap_or(5_000_000) as f64 / 1_000_000.0
        });
    }

    let mut provider_run = if let Some(id) = external_id {
        state.exa.get_agent_run(&id).await?
    } else {
        let created = state.exa.create_agent_run(request).await?;
        jobs::record_external_task(&state.pool, claim, &created.id).await?;
        created
    };
    while !provider_run.terminal() {
        tokio::time::sleep(POLL_INTERVAL).await;
        jobs::heartbeat(&state.pool, claim, owner, LEASE_SECONDS).await?;
        provider_run = state.exa.get_agent_run(&provider_run.id).await?;
    }

    let bytes = serde_json::to_vec(&provider_run)?;
    let (hash, path) = state.artifacts.put(&bytes).await?;
    let cost = provider_run.reported_cost_microusd();
    let reservation_id: String = sqlx::query_scalar(
        "SELECT id FROM spend WHERE operation_id=? AND state='reserved' ORDER BY created_at DESC LIMIT 1",
    )
    .bind(&claim.run_id)
    .fetch_one(&state.pool)
    .await?;
    budget::reconcile(
        &state.pool,
        &budget::Reservation { id: reservation_id },
        cost,
    )
    .await?;
    sqlx::query("INSERT INTO provider_runs (id,run_id,job_id,provider,external_task_id,status,stop_reason,output_hash,output_path,reported_microusd,collected_at) VALUES (?,?,?,?,?,?,?,?,?,?,?)")
        .bind(format!("provider_run_{}", uuid::Uuid::new_v4()))
        .bind(&claim.run_id)
        .bind(&claim.id)
        .bind("exa-agent")
        .bind(&provider_run.id)
        .bind(&provider_run.status)
        .bind(&provider_run.stop_reason)
        .bind(hash)
        .bind(path)
        .bind(cost)
        .bind(Utc::now().to_rfc3339())
        .execute(&state.pool)
        .await?;
    if provider_run.status == "completed" {
        jobs::complete(&state.pool, claim).await
    } else {
        jobs::fail(
            &state.pool,
            claim,
            &format!("Exa Agent ended with {}", provider_run.status),
        )
        .await
    }
}

pub async fn run(state: AppState) {
    let owner = format!("worker-{}", uuid::Uuid::new_v4());
    loop {
        if let Err(error) = jobs::reclaim_expired(&state.pool).await {
            tracing::error!(%error, "failed to reclaim expired jobs");
        }
        match process_one(&state, &owner).await {
            Ok(true) => {}
            Ok(false) => tokio::time::sleep(Duration::from_secs(1)).await,
            Err(error) => tracing::error!(%error, "worker job failed"),
        }
    }
}
