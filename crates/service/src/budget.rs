use chrono::{Datelike, SecondsFormat, Utc};
use sqlx::{SqliteConnection, SqlitePool};

use crate::error::AppError;

pub const DAILY_CAP_MICRO_USD: i64 = 15_000_000;
pub const MONTHLY_CAP_MICRO_USD: i64 = 150_000_000;
pub const EXA_SEARCH_RESERVATION_MICRO_USD: i64 = 10_000;
pub const EXA_CONTENTS_RESERVATION_MICRO_USD: i64 = 10_000;

#[derive(Debug)]
pub struct Reservation {
    pub id: String,
}

pub async fn reserve(
    pool: &SqlitePool,
    operation_kind: &str,
    operation_id: &str,
    amount: i64,
) -> Result<Reservation, AppError> {
    let mut connection = pool.acquire().await?;
    sqlx::query("BEGIN IMMEDIATE")
        .execute(&mut *connection)
        .await?;
    let result =
        reserve_in_transaction(&mut connection, operation_kind, operation_id, amount).await;
    match result {
        Ok(reservation) => {
            sqlx::query("COMMIT").execute(&mut *connection).await?;
            Ok(reservation)
        }
        Err(error) => {
            let _ = sqlx::query("ROLLBACK").execute(&mut *connection).await;
            Err(error)
        }
    }
}

async fn reserve_in_transaction(
    connection: &mut SqliteConnection,
    operation_kind: &str,
    operation_id: &str,
    amount: i64,
) -> Result<Reservation, AppError> {
    let now = Utc::now();
    let day = now
        .date_naive()
        .and_hms_opt(0, 0, 0)
        .expect("midnight is valid")
        .and_utc();
    let month = now
        .date_naive()
        .with_day(1)
        .expect("first day is valid")
        .and_hms_opt(0, 0, 0)
        .expect("midnight is valid")
        .and_utc();
    let day_total: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(reserved_microusd), 0) FROM spend WHERE state IN ('reserved','reconciled','unknown') AND created_at >= ?",
    )
    .bind(day.to_rfc3339())
    .fetch_one(&mut *connection)
    .await?;
    let month_total: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(reserved_microusd), 0) FROM spend WHERE state IN ('reserved','reconciled','unknown') AND created_at >= ?",
    )
    .bind(month.to_rfc3339())
    .fetch_one(&mut *connection)
    .await?;
    if day_total + amount > DAILY_CAP_MICRO_USD || month_total + amount > MONTHLY_CAP_MICRO_USD {
        return Err(AppError::forbidden(
            "spend_cap_exceeded",
            "provider spend reservation exceeds a configured cap",
        ));
    }
    let id = format!("spend_{}", uuid::Uuid::new_v4());
    sqlx::query(
        "INSERT INTO spend (id, operation_kind, operation_id, provider, reserved_microusd, state, idempotency_key, created_at) VALUES (?, ?, ?, 'exa', ?, 'reserved', ?, ?)",
    )
    .bind(&id)
    .bind(operation_kind)
    .bind(operation_id)
    .bind(amount)
    .bind(operation_id)
    .bind(now.to_rfc3339_opts(SecondsFormat::Millis, true))
    .execute(&mut *connection)
    .await?;
    Ok(Reservation { id })
}

pub async fn reconcile(
    pool: &SqlitePool,
    reservation: &Reservation,
    reported: Option<i64>,
) -> Result<(), AppError> {
    let state = if reported.is_some() {
        "reconciled"
    } else {
        "unknown"
    };
    sqlx::query(
        "UPDATE spend SET reserved_microusd = COALESCE(?, reserved_microusd), reported_microusd = ?, state = ?, reconciled_at = ? WHERE id = ?",
    )
    .bind(reported)
    .bind(reported)
    .bind(state)
    .bind(Utc::now().to_rfc3339())
    .bind(&reservation.id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn release(pool: &SqlitePool, reservation: &Reservation) -> Result<(), AppError> {
    sqlx::query("UPDATE spend SET state = 'released', reconciled_at = ? WHERE id = ?")
        .bind(Utc::now().to_rfc3339())
        .bind(&reservation.id)
        .execute(pool)
        .await?;
    Ok(())
}
