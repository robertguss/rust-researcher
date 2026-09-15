use std::time::Duration;

use chrono::Utc;
use research_protocol::{
    ArtifactRef, Assumptions, Claim, ClaimKind, ClaimReview, Completion, Evidence,
    EvidenceRelation, FreshnessClass, Label, ProducedBy, ReportEnvelope, ReportImportRequest,
    ReportSearch, ReportSource, RequiredQuestion, Review, ReviewLevel, SourceRequest,
    SourceResponse,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeSet, HashMap};
use unicode_normalization::UnicodeNormalization;

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
    request["systemPrompt"] = config.get("systemPrompt").cloned().unwrap_or_else(|| {
        Value::String("Return concise research with material claims separated. For every evidence item, copy an exact passage from the cited page; never paraphrase inside quote. Unsupported claims must have an empty evidence array.".into())
    });
    request["outputSchema"] = config
        .get("outputSchema")
        .cloned()
        .unwrap_or_else(default_output_schema);
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
        let sources = acquire_grounded_sources(state, claim, &provider_run).await?;
        publish_provider_draft(state, claim, &provider_run, &bytes, sources).await?;
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

fn default_output_schema() -> Value {
    json!({
        "type": "object",
        "required": ["answerMarkdown", "claims"],
        "properties": {
            "answerMarkdown": {"type": "string"},
            "claims": {
                "type": "array",
                "items": {
                    "type": "object",
                    "required": ["id", "kind", "text", "material", "evidence"],
                    "properties": {
                        "id": {"type": "string"},
                        "kind": {"type": "string", "enum": ["observation", "inference", "recommendation"]},
                        "text": {"type": "string"},
                        "material": {"type": "boolean"},
                        "evidence": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "required": ["url", "quote"],
                                "properties": {
                                    "url": {"type": "string"},
                                    "quote": {"type": "string"}
                                }
                            }
                        }
                    }
                }
            }
        }
    })
}

async fn acquire_grounded_sources(
    state: &AppState,
    claim: &jobs::JobClaim,
    provider_run: &AgentRun,
) -> Result<Vec<SourceResponse>, AppError> {
    let mut urls = provider_run
        .output
        .get("grounding")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|grounding| grounding.get("citations").and_then(Value::as_array))
        .flatten()
        .filter_map(|citation| citation.get("url").and_then(Value::as_str))
        .map(str::to_owned)
        .collect::<BTreeSet<_>>();
    if let Some(claims) = provider_run
        .output
        .get("structured")
        .and_then(|structured| structured.get("claims"))
        .and_then(Value::as_array)
    {
        for url in claims
            .iter()
            .filter_map(|claim| claim.get("evidence").and_then(Value::as_array))
            .flatten()
            .filter_map(|evidence| evidence.get("url").and_then(Value::as_str))
        {
            urls.insert(url.to_owned());
        }
    }
    let mut acquired = Vec::new();
    for url in urls {
        match crate::acquire_source(
            state,
            SourceRequest {
                url: url.clone(),
                freshness_class: FreshnessClass::Current,
                require_live: false,
            },
        )
        .await
        {
            Ok(source) => {
                sqlx::query("INSERT OR IGNORE INTO run_sources (run_id,acquisition_id,provenance,inclusion_reason) VALUES (?,?,'exa_grounding','provider citation')")
                    .bind(&claim.run_id)
                    .bind(&source.acquisition_id)
                    .execute(&state.pool)
                    .await?;
                acquired.push(source);
            }
            Err(error) => {
                sqlx::query("INSERT INTO events (run_id,event_type,occurred_at,payload_json) VALUES (?,'grounded_source_acquisition_failed',?,?)")
                    .bind(&claim.run_id)
                    .bind(Utc::now().to_rfc3339())
                    .bind(json!({"url": url, "error": error.to_string()}).to_string())
                    .execute(&state.pool)
                    .await?;
            }
        }
    }
    Ok(acquired)
}

async fn publish_provider_draft(
    state: &AppState,
    claim: &jobs::JobClaim,
    provider_run: &AgentRun,
    provider_bytes: &[u8],
    sources: Vec<SourceResponse>,
) -> Result<(), AppError> {
    let fallback_text = provider_run
        .output
        .get("text")
        .and_then(Value::as_str)
        .unwrap_or("Provider completed without textual output.");
    let structured = provider_run.output.get("structured");
    let answer = structured
        .and_then(|value| value.get("answerMarkdown"))
        .and_then(Value::as_str)
        .unwrap_or(fallback_text);
    let (brief, backend_config, context_version): (String, String, Option<i64>) = sqlx::query_as(
        "SELECT brief_json,backend_config_json,context_version FROM runs WHERE id=?",
    )
    .bind(&claim.run_id)
    .fetch_one(&state.pool)
    .await?;
    let mut source_by_url = HashMap::new();
    for (index, source) in sources.iter().enumerate() {
        if let Ok(url) = crate::acquisition::canonicalize(&source.canonical_url) {
            source_by_url.insert(url, index);
        }
        if let Ok(url) = crate::acquisition::canonicalize(&source.final_url) {
            source_by_url.insert(url, index);
        }
    }
    let mut evidence = Vec::new();
    let mut claims = Vec::new();
    if let Some(provider_claims) = structured
        .and_then(|value| value.get("claims"))
        .and_then(Value::as_array)
    {
        for (claim_index, provider_claim) in provider_claims.iter().enumerate() {
            let claim_id = format!("provider-claim-{}", claim_index + 1);
            let mut claim_evidence = Vec::new();
            if let Some(items) = provider_claim.get("evidence").and_then(Value::as_array) {
                for (evidence_index, item) in items.iter().enumerate() {
                    let Some(url) = item.get("url").and_then(Value::as_str) else {
                        continue;
                    };
                    let Some(quote) = item.get("quote").and_then(Value::as_str) else {
                        continue;
                    };
                    let Ok(canonical_url) = crate::acquisition::canonicalize(url) else {
                        continue;
                    };
                    let Some(source) = source_by_url
                        .get(&canonical_url)
                        .and_then(|index| sources.get(*index))
                    else {
                        continue;
                    };
                    if quote.trim().is_empty()
                        || !normalise(&source.text).contains(&normalise(quote))
                    {
                        continue;
                    }
                    let evidence_id = format!(
                        "provider-evidence-{}-{}",
                        claim_index + 1,
                        evidence_index + 1
                    );
                    claim_evidence.push(evidence_id.clone());
                    evidence.push(Evidence {
                        id: evidence_id,
                        extraction: source.extraction_id.clone(),
                        relation: EvidenceRelation::Supports,
                        locator: json!({"kind": "exact_quote", "url": canonical_url}),
                        quote: quote.into(),
                        normalisation: "whitespace,unicode-nfkc".into(),
                        mechanical_check: None,
                    });
                }
            }
            claims.push(Claim {
                id: claim_id,
                kind: match provider_claim.get("kind").and_then(Value::as_str) {
                    Some("recommendation") => ClaimKind::Recommendation,
                    Some("inference") => ClaimKind::Inference,
                    _ => ClaimKind::Observation,
                },
                text: provider_claim
                    .get("text")
                    .and_then(Value::as_str)
                    .unwrap_or("Provider emitted an empty claim")
                    .into(),
                material: provider_claim
                    .get("material")
                    .and_then(Value::as_bool)
                    .unwrap_or(true),
                evidence: claim_evidence,
                derivation: None,
                review: ClaimReview {
                    checked: false,
                    by: None,
                    note: Some(
                        "Exact quotations verified mechanically; semantic support not yet reviewed"
                            .into(),
                    ),
                    outcome: None,
                },
            });
        }
    }
    if claims.is_empty() {
        claims.push(Claim {
            id: "provider-claim-1".into(),
            kind: ClaimKind::Observation,
            text: fallback_text.into(),
            material: true,
            evidence: vec![],
            derivation: None,
            review: ClaimReview {
                checked: false,
                by: None,
                note: Some("Provider did not return structured claims".into()),
                outcome: None,
            },
        });
    }
    let markers = claims
        .iter()
        .map(|claim| format!("[^{}]", claim.id))
        .collect::<Vec<_>>()
        .join(" ");
    let body = format!("{answer}\n\n<!-- {markers} -->");

    let mut report_sources = Vec::new();
    for source in sources {
        let (retrieved_at, origin_group): (String, String) = sqlx::query_as(
            "SELECT a.retrieved_at,s.origin_group FROM acquisitions a JOIN sources s ON s.id=a.source_id WHERE a.id=?",
        )
        .bind(&source.acquisition_id)
        .fetch_one(&state.pool)
        .await?;
        report_sources.push(ReportSource {
            acquisition: source.acquisition_id,
            extraction: source.extraction_id,
            content_kind: source.content_kind,
            access_level: source.access_level,
            retrieved_at: chrono::DateTime::parse_from_rfc3339(&retrieved_at)
                .map_err(|_| {
                    AppError::validation("invalid_timestamp", "stored timestamp is invalid")
                })?
                .with_timezone(&Utc),
            origin_group,
        });
    }
    let body_hash = sha256(body.as_bytes());
    let provider_native: Value = serde_json::from_slice(provider_bytes)?;
    let provider_hash = sha256(&serde_json::to_vec(&provider_native)?);
    let envelope = ReportEnvelope {
        schema_version: "1".into(),
        run_id: claim.run_id.clone(),
        revision: 1,
        supersedes: None,
        label: Some(Label::Reviewed),
        produced_by: ProducedBy {
            backend: "exa-agent".into(),
            backend_config: serde_json::from_str(&backend_config)?,
            instruction_version: "2026-09-15.1".into(),
            context_version: context_version.unwrap_or_default() as u32,
            model: None,
        },
        brief: serde_json::from_str(&brief)?,
        assumptions: Assumptions::default(),
        body_markdown_path: "report.md".into(),
        body_hash,
        claims,
        evidence,
        assessments: vec![],
        sources: report_sources,
        searches: Vec::<ReportSearch>::new(),
        completion: Completion {
            required_questions: vec![RequiredQuestion {
                q: "Material claims checked against acquired passages".into(),
                answered: false,
                unanswerable_reason: None,
            }],
            hit_limit: None,
            what_would_change_this: "Review each material claim against acquired source text"
                .into(),
        },
        review: Review {
            level: ReviewLevel::Structural,
            records: vec![],
        },
        provider_native: Some(ArtifactRef {
            path: "provider-native.json".into(),
            hash: provider_hash,
        }),
        artifacts: Default::default(),
    };
    crate::reports::import(
        &state.pool,
        &state.artifacts,
        ReportImportRequest {
            envelope,
            body_markdown: body,
            provider_native: Some(provider_native),
        },
    )
    .await?;
    Ok(())
}

fn sha256(value: &[u8]) -> String {
    format!("sha256:{}", hex::encode(Sha256::digest(value)))
}

fn normalise(value: &str) -> String {
    value
        .nfkc()
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
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
