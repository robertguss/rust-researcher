use async_trait::async_trait;
use research_protocol::SearchResult;
use serde_json::{Value, json};
use std::collections::BTreeMap;

use crate::error::AppError;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AgentRun {
    pub id: String,
    pub status: String,
    #[serde(rename = "stopReason")]
    pub stop_reason: Option<String>,
    #[serde(default)]
    pub output: Value,
    #[serde(rename = "costDollars", default)]
    pub cost_dollars: Value,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

impl AgentRun {
    pub fn reported_cost_microusd(&self) -> Option<i64> {
        self.cost_dollars
            .get("total")
            .and_then(Value::as_f64)
            .map(|dollars| (dollars * 1_000_000.0).round() as i64)
    }

    pub fn terminal(&self) -> bool {
        matches!(self.status.as_str(), "completed" | "failed" | "cancelled")
    }
}

#[derive(Debug)]
pub struct ProviderResponse<T> {
    pub value: T,
    pub reported_cost_microusd: Option<i64>,
}

#[derive(Debug)]
pub struct ExaContent {
    pub text: String,
    pub title: Option<String>,
}

#[async_trait]
pub trait ExaProvider: Send + Sync {
    async fn search(
        &self,
        query: &str,
        result_count: u32,
    ) -> Result<ProviderResponse<Vec<SearchResult>>, AppError>;
    async fn contents(&self, url: &str) -> Result<ProviderResponse<Option<ExaContent>>, AppError>;
    async fn create_agent_run(&self, request: Value) -> Result<AgentRun, AppError> {
        let _ = request;
        Err(AppError::validation(
            "agent_not_supported",
            "provider does not support Exa Agent",
        ))
    }
    async fn get_agent_run(&self, id: &str) -> Result<AgentRun, AppError> {
        let _ = id;
        Err(AppError::validation(
            "agent_not_supported",
            "provider does not support Exa Agent",
        ))
    }
    async fn cancel_agent_run(&self, id: &str) -> Result<AgentRun, AppError> {
        let _ = id;
        Err(AppError::validation(
            "agent_not_supported",
            "provider does not support Exa Agent",
        ))
    }
}

pub struct HttpExaProvider {
    client: reqwest::Client,
    api_key: String,
    base_url: String,
}

impl HttpExaProvider {
    pub fn new(api_key: String, base_url: String) -> Result<Self, AppError> {
        Ok(Self {
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(20))
                .build()?,
            api_key,
            base_url,
        })
    }

    async fn post(&self, path: &str, body: Value) -> Result<Value, AppError> {
        let response = self
            .client
            .post(format!("{}{path}", self.base_url.trim_end_matches('/')))
            .header("x-api-key", &self.api_key)
            .json(&body)
            .send()
            .await?;
        if !response.status().is_success() {
            return Err(AppError::Client {
                status: axum::http::StatusCode::BAD_GATEWAY,
                code: "exa_error",
                message: format!("Exa returned HTTP {}", response.status()),
                retryable: response.status().is_server_error(),
            });
        }
        Ok(response.json().await?)
    }

    async fn get(&self, path: &str) -> Result<Value, AppError> {
        let response = self
            .client
            .get(format!("{}{path}", self.base_url.trim_end_matches('/')))
            .header("x-api-key", &self.api_key)
            .send()
            .await?;
        if !response.status().is_success() {
            return Err(exa_http_error(response.status()));
        }
        Ok(response.json().await?)
    }
}

fn exa_http_error(status: reqwest::StatusCode) -> AppError {
    AppError::Client {
        status: axum::http::StatusCode::BAD_GATEWAY,
        code: "exa_error",
        message: format!("Exa returned HTTP {status}"),
        retryable: status.is_server_error() || status.as_u16() == 429,
    }
}

fn reported_cost(value: &Value) -> Option<i64> {
    value
        .pointer("/costDollars/total")
        .or_else(|| value.get("costDollars"))
        .and_then(Value::as_f64)
        .map(|dollars| (dollars * 1_000_000.0).round() as i64)
}

#[async_trait]
impl ExaProvider for HttpExaProvider {
    async fn search(
        &self,
        query: &str,
        result_count: u32,
    ) -> Result<ProviderResponse<Vec<SearchResult>>, AppError> {
        let value = self
            .post(
                "/search",
                json!({ "query": query, "numResults": result_count }),
            )
            .await?;
        let results = value
            .get("results")
            .and_then(Value::as_array)
            .ok_or_else(|| {
                AppError::validation(
                    "exa_schema_changed",
                    "Exa search response has no results array",
                )
            })?
            .iter()
            .filter_map(|item| {
                Some(SearchResult {
                    url: item.get("url")?.as_str()?.to_owned(),
                    title: item.get("title").and_then(Value::as_str).map(str::to_owned),
                    published_date: item
                        .get("publishedDate")
                        .and_then(Value::as_str)
                        .map(str::to_owned),
                    exclusion_reason: None,
                })
            })
            .collect();
        Ok(ProviderResponse {
            value: results,
            reported_cost_microusd: reported_cost(&value),
        })
    }

    async fn contents(&self, url: &str) -> Result<ProviderResponse<Option<ExaContent>>, AppError> {
        let value = self
            .post("/contents", json!({ "urls": [url], "text": true }))
            .await?;
        let content = value
            .get("results")
            .and_then(Value::as_array)
            .and_then(|results| results.first())
            .and_then(|item| {
                Some(ExaContent {
                    text: item.get("text")?.as_str()?.to_owned(),
                    title: item.get("title").and_then(Value::as_str).map(str::to_owned),
                })
            });
        Ok(ProviderResponse {
            value: content,
            reported_cost_microusd: reported_cost(&value),
        })
    }

    async fn create_agent_run(&self, request: Value) -> Result<AgentRun, AppError> {
        Ok(serde_json::from_value(
            self.post("/agent/runs", request).await?,
        )?)
    }

    async fn get_agent_run(&self, id: &str) -> Result<AgentRun, AppError> {
        Ok(serde_json::from_value(
            self.get(&format!("/agent/runs/{id}")).await?,
        )?)
    }

    async fn cancel_agent_run(&self, id: &str) -> Result<AgentRun, AppError> {
        Ok(serde_json::from_value(
            self.post(&format!("/agent/runs/{id}/cancel"), json!({}))
                .await?,
        )?)
    }
}
