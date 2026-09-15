use chrono::{Duration, Utc};
use sqlx::SqlitePool;

use crate::error::AppError;

#[derive(Debug, Clone)]
pub struct JobClaim {
    pub id: String,
    pub run_id: String,
    pub attempt_epoch: i64,
}

pub async fn claim_next(
    pool: &SqlitePool,
    lane: &str,
    owner: &str,
    lease_seconds: i64,
) -> Result<Option<JobClaim>, AppError> {
    if !matches!(lane, "control" | "heavy") || lease_seconds <= 0 {
        return Err(AppError::validation(
            "invalid_claim",
            "valid lane and positive lease are required",
        ));
    }
    let now = Utc::now();
    let expires = now + Duration::seconds(lease_seconds);
    let mut tx = pool.begin().await?;
    let claim: Option<(String, String, i64)> = sqlx::query_as(
        "UPDATE jobs SET state='running',attempt=attempt+1,attempt_epoch=attempt_epoch+1,lease_owner=?,lease_expires_at=?,updated_at=? WHERE id=(SELECT id FROM jobs WHERE lane=? AND (state='queued' OR (state='retry_wait' AND retry_at<=?)) ORDER BY created_at,id LIMIT 1) RETURNING id,run_id,attempt_epoch",
    )
    .bind(owner)
    .bind(expires.to_rfc3339())
    .bind(now.to_rfc3339())
    .bind(lane)
    .bind(now.to_rfc3339())
    .fetch_optional(&mut *tx)
    .await?;
    if let Some((_, run_id, _)) = &claim {
        sqlx::query(
            "UPDATE runs SET execution='running',updated_at=? WHERE id=? AND execution='queued'",
        )
        .bind(now.to_rfc3339())
        .bind(run_id)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(claim.map(|(id, run_id, attempt_epoch)| JobClaim {
        id,
        run_id,
        attempt_epoch,
    }))
}

pub async fn heartbeat(
    pool: &SqlitePool,
    claim: &JobClaim,
    owner: &str,
    lease_seconds: i64,
) -> Result<(), AppError> {
    let changed = sqlx::query("UPDATE jobs SET lease_expires_at=?,updated_at=? WHERE id=? AND state='running' AND attempt_epoch=? AND lease_owner=?")
        .bind((Utc::now() + Duration::seconds(lease_seconds)).to_rfc3339())
        .bind(Utc::now().to_rfc3339())
        .bind(&claim.id)
        .bind(claim.attempt_epoch)
        .bind(owner)
        .execute(pool).await?.rows_affected();
    require_fence(changed)
}

pub async fn record_external_task(
    pool: &SqlitePool,
    claim: &JobClaim,
    external_task_id: &str,
) -> Result<(), AppError> {
    let mut tx = pool.begin().await?;
    let changed = sqlx::query("UPDATE jobs SET external_task_id=?,updated_at=? WHERE id=? AND state='running' AND attempt_epoch=?")
        .bind(external_task_id).bind(Utc::now().to_rfc3339()).bind(&claim.id).bind(claim.attempt_epoch)
        .execute(&mut *tx).await?.rows_affected();
    require_fence(changed)?;
    sqlx::query("UPDATE runs SET external='accepted',updated_at=? WHERE id=?")
        .bind(Utc::now().to_rfc3339())
        .bind(&claim.run_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}

pub async fn complete(pool: &SqlitePool, claim: &JobClaim) -> Result<(), AppError> {
    let mut tx = pool.begin().await?;
    let changed = sqlx::query("UPDATE jobs SET state='succeeded',lease_owner=NULL,lease_expires_at=NULL,updated_at=? WHERE id=? AND state='running' AND attempt_epoch=?")
        .bind(Utc::now().to_rfc3339()).bind(&claim.id).bind(claim.attempt_epoch)
        .execute(&mut *tx).await?.rows_affected();
    require_fence(changed)?;
    sqlx::query("UPDATE runs SET execution='succeeded',external=CASE WHEN external='none' THEN 'none' ELSE 'terminal' END,updated_at=? WHERE id=?")
        .bind(Utc::now().to_rfc3339()).bind(&claim.run_id).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(())
}

pub async fn fail(pool: &SqlitePool, claim: &JobClaim, error: &str) -> Result<(), AppError> {
    let now = Utc::now().to_rfc3339();
    let mut tx = pool.begin().await?;
    let changed = sqlx::query("UPDATE jobs SET state='failed',error_json=?,lease_owner=NULL,lease_expires_at=NULL,updated_at=? WHERE id=? AND state='running' AND attempt_epoch=?")
        .bind(serde_json::json!({"message": error}).to_string())
        .bind(&now)
        .bind(&claim.id)
        .bind(claim.attempt_epoch)
        .execute(&mut *tx)
        .await?
        .rows_affected();
    require_fence(changed)?;
    sqlx::query("UPDATE runs SET execution='failed',external=CASE WHEN external='none' THEN 'none' ELSE 'terminal' END,updated_at=? WHERE id=?")
        .bind(&now)
        .bind(&claim.run_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}

pub async fn reclaim_expired(pool: &SqlitePool) -> Result<u64, AppError> {
    let now = Utc::now().to_rfc3339();
    let mut tx = pool.begin().await?;
    let ambiguous = sqlx::query("UPDATE jobs SET state='unknown',lease_owner=NULL,lease_expires_at=NULL,updated_at=? WHERE state='running' AND lease_expires_at<? AND external_task_id IS NOT NULL")
        .bind(&now).bind(&now).execute(&mut *tx).await?.rows_affected();
    sqlx::query("UPDATE runs SET execution='blocked',blocked_reason='reconcile',external='unreconciled',updated_at=? WHERE id IN (SELECT run_id FROM jobs WHERE state='unknown')")
        .bind(&now).execute(&mut *tx).await?;
    let safe = sqlx::query("UPDATE jobs SET state='queued',lease_owner=NULL,lease_expires_at=NULL,updated_at=? WHERE state='running' AND lease_expires_at<? AND external_task_id IS NULL")
        .bind(&now).bind(&now).execute(&mut *tx).await?.rows_affected();
    tx.commit().await?;
    Ok(ambiguous + safe)
}

fn require_fence(changed: u64) -> Result<(), AppError> {
    if changed == 1 {
        Ok(())
    } else {
        Err(AppError::conflict(
            "attempt_epoch_mismatch",
            "job write rejected because this attempt no longer owns the job",
        ))
    }
}
