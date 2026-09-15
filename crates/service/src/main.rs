use std::net::SocketAddr;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    let database_url =
        std::env::var("DATABASE_URL").unwrap_or_else(|_| "sqlite://research.db".into());
    let artifact_root = std::env::var("ARTIFACT_ROOT").unwrap_or_else(|_| "artifacts".into());
    let token = std::env::var("RESEARCH_API_TOKEN")
        .map_err(|_| anyhow::anyhow!("RESEARCH_API_TOKEN is required"))?;
    let state = research_service::AppState::open(
        &database_url,
        artifact_root,
        token,
        research_service::live_exa_from_env()?,
    )
    .await?;
    tokio::spawn(research_service::worker::run(state.clone()));
    let address: SocketAddr = std::env::var("RESEARCH_LISTEN")
        .unwrap_or_else(|_| "127.0.0.1:3000".into())
        .parse()?;
    let listener = tokio::net::TcpListener::bind(address).await?;
    tracing::info!(%address, "research service listening");
    axum::serve(listener, research_service::router(state)).await?;
    Ok(())
}
