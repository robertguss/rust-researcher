use chrono::{DateTime, Utc};
use research_protocol::{ContextDocument, Scope};
use sqlx::SqlitePool;

use crate::error::AppError;

pub async fn latest(pool: &SqlitePool, scope: &Scope) -> Result<Option<ContextDocument>, AppError> {
    let row: Option<(i64, String, String)> = sqlx::query_as(
        "SELECT version,document_json,created_at FROM context WHERE scope=? ORDER BY version DESC LIMIT 1",
    )
    .bind(scope_name(scope))
    .fetch_optional(pool)
    .await?;
    row.map(|(version, document, created_at)| {
        Ok(ContextDocument {
            scope: scope.clone(),
            version: version as u32,
            document: serde_json::from_str(&document)?,
            created_at: DateTime::parse_from_rfc3339(&created_at)
                .map_err(|_| {
                    AppError::validation("invalid_timestamp", "stored timestamp is invalid")
                })?
                .with_timezone(&Utc),
        })
    })
    .transpose()
}

pub async fn set(
    pool: &SqlitePool,
    scope: Scope,
    document: serde_json::Value,
) -> Result<ContextDocument, AppError> {
    if !document.is_object() {
        return Err(AppError::validation(
            "invalid_context",
            "context document must be an object",
        ));
    }
    let mut connection = pool.acquire().await?;
    sqlx::query("BEGIN IMMEDIATE")
        .execute(&mut *connection)
        .await?;
    let result = async {
        let previous: i64 =
            sqlx::query_scalar("SELECT COALESCE(MAX(version),0) FROM context WHERE scope=?")
                .bind(scope_name(&scope))
                .fetch_one(&mut *connection)
                .await?;
        let version = previous + 1;
        let now = Utc::now();
        sqlx::query(
            "INSERT INTO context (scope,version,document_json,created_at) VALUES (?,?,?,?)",
        )
        .bind(scope_name(&scope))
        .bind(version)
        .bind(serde_json::to_string(&document)?)
        .bind(now.to_rfc3339())
        .execute(&mut *connection)
        .await?;
        Ok(ContextDocument {
            scope,
            version: version as u32,
            document,
            created_at: now,
        })
    }
    .await;
    match result {
        Ok(context) => {
            sqlx::query("COMMIT").execute(&mut *connection).await?;
            Ok(context)
        }
        Err(error) => {
            let _ = sqlx::query("ROLLBACK").execute(&mut *connection).await;
            Err(error)
        }
    }
}

pub fn scope_name(scope: &Scope) -> &'static str {
    match scope {
        Scope::Personal => "personal",
        Scope::Work => "work",
    }
}
