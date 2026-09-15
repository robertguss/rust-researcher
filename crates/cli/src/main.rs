use anyhow::{Context, bail};
use clap::{Parser, Subcommand};
use research_protocol::{
    FreshnessClass, ReportEnvelope, ReportImportRequest, SearchRequest, SourceRequest,
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
    Backends,
    Summary,
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
