use anyhow::{Context, bail};
use clap::{Parser, Subcommand};
use research_protocol::{
    Clarification, Classification, CreateRunRequest, Depth, FreshnessClass, ReconcileRequest,
    ReportEnvelope, ReportImportRequest, ResearchBrief, Scope, SearchRequest, SourceRequest,
};
use serde::{Serialize, de::DeserializeOwned};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "research", about = "Thin client for the rust-researcher API")]
struct Cli {
    #[arg(
        long,
        env = "RESEARCH_API_URL",
        default_value = "http://127.0.0.1:3000"
    )]
    api_url: String,
    #[arg(long, env = "RESEARCH_API_TOKEN", hide_env_values = true)]
    token: String,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Search {
        query: String,
        #[arg(long, default_value_t = 10)]
        result_count: u32,
    },
    Source {
        source_or_url: String,
        #[arg(long)]
        live: bool,
        #[arg(long, value_enum, default_value_t = CliFreshness::Stable)]
        freshness: CliFreshness,
    },
    Import {
        file: PathBuf,
    },
    Run {
        question: String,
        #[arg(long)]
        backend: String,
        #[arg(long, default_value = "standard")]
        depth: CliDepth,
        #[arg(long)]
        effort: Option<String>,
        #[arg(long)]
        max_cost: Option<String>,
        #[arg(long)]
        max_duration: Option<u64>,
        #[arg(long)]
        accept_weaker_limits: bool,
        #[arg(long)]
        idempotency_key: Option<String>,
    },
    Status {
        run_id: String,
    },
    Result {
        run_id: String,
    },
    Report {
        report_id: String,
    },
    Review {
        report_id: String,
        file: PathBuf,
    },
    Cancel {
        run_id: String,
    },
    Reconcile {
        run_id: String,
        #[arg(value_enum)]
        action: ReconcileAction,
        #[arg(long)]
        external_task_id: Option<String>,
        #[arg(long)]
        accept_charge: bool,
    },
    Backends,
    Summary,
}

#[derive(Clone, clap::ValueEnum)]
enum ReconcileAction {
    Adopt,
    MarkFailed,
    Resubmit,
}

#[derive(Clone, clap::ValueEnum)]
enum CliDepth {
    Lookup,
    Standard,
    Deep,
    Extended,
}

impl From<CliDepth> for Depth {
    fn from(value: CliDepth) -> Self {
        match value {
            CliDepth::Lookup => Self::Lookup,
            CliDepth::Standard => Self::Standard,
            CliDepth::Deep => Self::Deep,
            CliDepth::Extended => Self::Extended,
        }
    }
}

#[derive(Clone, clap::ValueEnum, Default)]
enum CliFreshness {
    Volatile,
    Current,
    #[default]
    Stable,
    Immutable,
}

impl From<CliFreshness> for FreshnessClass {
    fn from(value: CliFreshness) -> Self {
        match value {
            CliFreshness::Volatile => Self::Volatile,
            CliFreshness::Current => Self::Current,
            CliFreshness::Stable => Self::Stable,
            CliFreshness::Immutable => Self::Immutable,
        }
    }
}

struct Client {
    http: reqwest::Client,
    base: String,
    token: String,
}

impl Client {
    async fn post<T: Serialize, R: DeserializeOwned>(
        &self,
        path: &str,
        body: &T,
    ) -> anyhow::Result<R> {
        self.handle(
            self.http
                .post(format!("{}{path}", self.base.trim_end_matches('/')))
                .bearer_auth(&self.token)
                .json(body)
                .send()
                .await?,
        )
        .await
    }

    async fn post_idempotent<T: Serialize, R: DeserializeOwned>(
        &self,
        path: &str,
        key: &str,
        body: &T,
    ) -> anyhow::Result<R> {
        self.handle(
            self.http
                .post(format!("{}{path}", self.base.trim_end_matches('/')))
                .bearer_auth(&self.token)
                .header("idempotency-key", key)
                .json(body)
                .send()
                .await?,
        )
        .await
    }

    async fn get<R: DeserializeOwned>(&self, path: &str) -> anyhow::Result<R> {
        self.handle(
            self.http
                .get(format!("{}{path}", self.base.trim_end_matches('/')))
                .bearer_auth(&self.token)
                .send()
                .await?,
        )
        .await
    }

    async fn handle<R: DeserializeOwned>(&self, response: reqwest::Response) -> anyhow::Result<R> {
        let status = response.status();
        let bytes = response.bytes().await?;
        if !status.is_success() {
            let detail = serde_json::from_slice::<research_protocol::ErrorResponse>(&bytes)
                .map(|error| {
                    format!(
                        "{}: {} (request {})",
                        error.code, error.message, error.request_id
                    )
                })
                .unwrap_or_else(|_| String::from_utf8_lossy(&bytes).into_owned());
            bail!("HTTP {status}: {detail}");
        }
        serde_json::from_slice(&bytes).context("API returned invalid JSON")
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let client = Client {
        http: reqwest::Client::new(),
        base: cli.api_url,
        token: cli.token,
    };
    let output = match cli.command {
        Command::Search {
            query,
            result_count,
        } => {
            let response: research_protocol::SearchResponse = client
                .post(
                    "/v1/search",
                    &SearchRequest {
                        query,
                        result_count,
                    },
                )
                .await?;
            serde_json::to_value(response)?
        }
        Command::Source {
            source_or_url,
            live,
            freshness,
        } => {
            let response: research_protocol::SourceResponse =
                if source_or_url.starts_with("http://") || source_or_url.starts_with("https://") {
                    client
                        .post(
                            "/v1/sources",
                            &SourceRequest {
                                url: source_or_url,
                                freshness_class: freshness.into(),
                                require_live: live,
                            },
                        )
                        .await?
                } else {
                    client.get(&format!("/v1/sources/{source_or_url}")).await?
                };
            serde_json::to_value(response)?
        }
        Command::Import { file } => {
            let bytes = tokio::fs::read(&file)
                .await
                .with_context(|| format!("reading {}", file.display()))?;
            let envelope: ReportEnvelope =
                serde_json::from_slice(&bytes).context("parsing report envelope")?;
            let directory = file.parent().unwrap_or_else(|| std::path::Path::new("."));
            let body_markdown =
                tokio::fs::read_to_string(directory.join(&envelope.body_markdown_path))
                    .await
                    .context("reading report Markdown")?;
            let provider_native = if let Some(reference) = &envelope.provider_native {
                let bytes = tokio::fs::read(directory.join(&reference.path))
                    .await
                    .context("reading provider-native artifact")?;
                Some(serde_json::from_slice(&bytes).context("parsing provider-native JSON")?)
            } else {
                None
            };
            let response: research_protocol::ReportImportResponse = client
                .post(
                    "/v1/reports/import",
                    &ReportImportRequest {
                        envelope,
                        body_markdown,
                        provider_native,
                    },
                )
                .await?;
            serde_json::to_value(response)?
        }
        Command::Run {
            question,
            backend,
            depth,
            effort,
            max_cost,
            max_duration,
            accept_weaker_limits,
            idempotency_key,
        } => {
            let key = idempotency_key.unwrap_or_else(|| format!("cli_{}", uuid::Uuid::new_v4()));
            let request = CreateRunRequest {
                brief: ResearchBrief {
                    question,
                    decision: "unknown".into(),
                    audience: None,
                    locale: None,
                    as_of: chrono::Utc::now(),
                    required_questions: vec![],
                    constraints: serde_json::Value::Null,
                    exclusions: serde_json::Value::Null,
                    assumptions: serde_json::Value::Null,
                    depth: depth.into(),
                    clarification: Clarification::Assume,
                    scope: Scope::Personal,
                    classification: Some(Classification::PersonalSensitive),
                    evidence_policy: serde_json::Value::Null,
                    output: serde_json::Value::Null,
                },
                backend,
                backend_config: effort
                    .map(|effort| serde_json::json!({ "effort": effort }))
                    .unwrap_or_else(|| serde_json::json!({})),
                max_duration_seconds: max_duration,
                max_cost_usd: max_cost,
                accept_weaker_limits,
                follow_up_of: None,
                classification_override_reason: None,
            };
            let response: research_protocol::RunResponse =
                client.post_idempotent("/v1/runs", &key, &request).await?;
            serde_json::to_value(response)?
        }
        Command::Status { run_id } => {
            let response: research_protocol::RunResponse =
                client.get(&format!("/v1/runs/{run_id}")).await?;
            serde_json::to_value(response)?
        }
        Command::Result { run_id } => {
            let response: research_protocol::ProviderRunResponse = client
                .get(&format!("/v1/runs/{run_id}/provider-result"))
                .await?;
            serde_json::to_value(response)?
        }
        Command::Report { report_id } => {
            let response: research_protocol::StoredReportResponse =
                client.get(&format!("/v1/reports/{report_id}")).await?;
            serde_json::to_value(response)?
        }
        Command::Review { report_id, file } => {
            let request: research_protocol::ReviewReportRequest =
                serde_json::from_slice(&tokio::fs::read(&file).await?)?;
            let response: research_protocol::ReportImportResponse = client
                .post(&format!("/v1/reports/{report_id}/review"), &request)
                .await?;
            serde_json::to_value(response)?
        }
        Command::Cancel { run_id } => {
            let response: research_protocol::RunResponse = client
                .post(&format!("/v1/runs/{run_id}/cancel"), &serde_json::json!({}))
                .await?;
            serde_json::to_value(response)?
        }
        Command::Reconcile {
            run_id,
            action,
            external_task_id,
            accept_charge,
        } => {
            let request = match action {
                ReconcileAction::Adopt => ReconcileRequest::Adopt {
                    external_task_id: external_task_id
                        .context("--external-task-id is required for adopt")?,
                },
                ReconcileAction::MarkFailed => ReconcileRequest::MarkFailed,
                ReconcileAction::Resubmit => ReconcileRequest::Resubmit { accept_charge },
            };
            let response: research_protocol::RunResponse = client
                .post(&format!("/v1/runs/{run_id}/reconcile"), &request)
                .await?;
            serde_json::to_value(response)?
        }
        Command::Backends => {
            let response: Vec<research_protocol::BackendContract> =
                client.get("/v1/backends").await?;
            serde_json::to_value(response)?
        }
        Command::Summary => {
            let response: research_protocol::SummaryResponse = client.get("/v1/summary").await?;
            serde_json::to_value(response)?
        }
    };
    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}
