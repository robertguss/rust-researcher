use axum::{Json, http::StatusCode, response::IntoResponse};
use research_protocol::ErrorResponse;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("{code}: {message}")]
    Client {
        status: StatusCode,
        code: &'static str,
        message: String,
        retryable: bool,
    },
    #[error(transparent)]
    Sql(#[from] sqlx::Error),
    #[error(transparent)]
    Migrate(#[from] sqlx::migrate::MigrateError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Http(#[from] reqwest::Error),
    #[error(transparent)]
    Url(#[from] url::ParseError),
}

impl AppError {
    pub fn validation(code: &'static str, message: impl Into<String>) -> Self {
        Self::Client {
            status: StatusCode::UNPROCESSABLE_ENTITY,
            code,
            message: message.into(),
            retryable: false,
        }
    }

    pub fn forbidden(code: &'static str, message: impl Into<String>) -> Self {
        Self::Client {
            status: StatusCode::FORBIDDEN,
            code,
            message: message.into(),
            retryable: false,
        }
    }

    pub fn conflict(code: &'static str, message: impl Into<String>) -> Self {
        Self::Client {
            status: StatusCode::CONFLICT,
            code,
            message: message.into(),
            retryable: false,
        }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> axum::response::Response {
        let request_id = format!("req_{}", uuid::Uuid::new_v4());
        let (status, code, message, retryable) = match self {
            Self::Client {
                status,
                code,
                message,
                retryable,
            } => (status, code, message, retryable),
            error => {
                tracing::error!(%request_id, error = %error, "request failed");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal_error",
                    "internal server error".into(),
                    true,
                )
            }
        };
        (
            status,
            Json(ErrorResponse {
                code: code.into(),
                message,
                retryable,
                request_id,
            }),
        )
            .into_response()
    }
}
