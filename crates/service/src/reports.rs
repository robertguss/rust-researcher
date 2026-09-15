use chrono::Utc;
use research_protocol::{
    AssessmentOutcome, ClaimKind, EvidenceRelation, Label, MechanicalCheck, ReportEnvelope,
    ReportImportRequest, ReportImportResponse, ReviewLevel,
};
use sha2::{Digest, Sha256};
use sqlx::SqlitePool;
use std::collections::HashSet;
use unicode_normalization::UnicodeNormalization;

use crate::{error::AppError, store::ArtifactStore};

const POLICY_VERSION: &str = "2026-09-14.1";

pub async fn import(
    pool: &SqlitePool,
    store: &ArtifactStore,
    mut request: ReportImportRequest,
) -> Result<ReportImportResponse, AppError> {
    validate_structure(pool, &request).await?;
    let body_hash = sha256(request.body_markdown.as_bytes());
    if request.envelope.body_hash != body_hash {
        return Err(AppError::validation(
            "body_hash_mismatch",
            "body_hash does not match attached Markdown",
        ));
    }
    let evidence_checks = check_evidence(pool, store, &request.envelope).await?;
    for (evidence, check) in request.envelope.evidence.iter_mut().zip(&evidence_checks) {
        evidence.mechanical_check = Some(check.clone());
    }
    if evidence_checks
        .iter()
        .any(|check| matches!(check, MechanicalCheck::Failed))
    {
        return Err(AppError::validation(
            "mechanical_check_failed",
            "one or more quotations were not found in their extraction",
        ));
    }

    let (label, reasons) = compute_label(&request.envelope);
    request.envelope.label = Some(label.clone());
    request
        .envelope
        .assessments
        .push(research_protocol::Assessment {
            at: Utc::now(),
            by: "service".into(),
            policy_version: POLICY_VERSION.into(),
            label: label.clone(),
            reasons: reasons.clone(),
        });
    let (stored_body_hash, body_path) = store.put(request.body_markdown.as_bytes()).await?;
    debug_assert_eq!(stored_body_hash, body_hash);
    let provider_artifact = if let Some(native) = &request.provider_native {
        let bytes = serde_json::to_vec(native)?;
        let (hash, path) = store.put(&bytes).await?;
        Some(research_protocol::ArtifactRef { path, hash })
    } else {
        None
    };
    request.envelope.provider_native = provider_artifact;
    request.envelope.artifacts.clear();
    request.envelope.artifacts.insert(
        "md".into(),
        research_protocol::ArtifactRef {
            path: body_path.clone(),
            hash: body_hash.clone(),
        },
    );
    let envelope_bytes = serde_json::to_vec_pretty(&request.envelope)?;
    let (envelope_hash, envelope_path) = store.put(&envelope_bytes).await?;
    let report_id = format!("report_{}", uuid::Uuid::new_v4());
    let mut tx = pool.begin().await?;
    for evidence in &request.envelope.evidence {
        sqlx::query(
            "INSERT INTO evidence (id, run_id, extraction_id, relation, locator_json, quoted_text, normalisation, mechanical_check) VALUES (?, ?, ?, ?, ?, ?, ?, 'passed')",
        )
        .bind(&evidence.id)
        .bind(&request.envelope.run_id)
        .bind(&evidence.extraction)
        .bind(relation_name(&evidence.relation))
        .bind(serde_json::to_string(&evidence.locator)?)
        .bind(&evidence.quote)
        .bind(&evidence.normalisation)
        .execute(&mut *tx)
        .await?;
    }
    sqlx::query(
        "INSERT INTO reports (id, run_id, revision, supersedes, envelope_hash, envelope_path, body_hash, body_path, current_computed_label, review_level, artifact_manifest_json, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&report_id)
    .bind(&request.envelope.run_id)
    .bind(request.envelope.revision)
    .bind(request.envelope.supersedes)
    .bind(envelope_hash)
    .bind(envelope_path)
    .bind(body_hash)
    .bind(body_path)
    .bind(label_name(&label))
    .bind(review_level_name(&request.envelope.review.level))
    .bind(serde_json::to_string(&request.envelope.artifacts)?)
    .bind(Utc::now().to_rfc3339())
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "INSERT INTO assessments (report_id, assessed_at, reviewer, policy_version, computed_label, reasons_json) VALUES (?, ?, 'service', ?, ?, ?)",
    )
    .bind(&report_id)
    .bind(Utc::now().to_rfc3339())
    .bind(POLICY_VERSION)
    .bind(label_name(&label))
    .bind(serde_json::to_string(&reasons)?)
    .execute(&mut *tx)
    .await?;
    sqlx::query("INSERT INTO events (run_id, event_type, occurred_at, payload_json) VALUES (?, 'report_imported', ?, ?)")
        .bind(&request.envelope.run_id)
        .bind(Utc::now().to_rfc3339())
        .bind(serde_json::json!({ "report_id": report_id, "revision": request.envelope.revision }).to_string())
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(ReportImportResponse {
        report_id,
        run_id: request.envelope.run_id,
        revision: request.envelope.revision,
        label,
        reasons,
    })
}

async fn validate_structure(
    pool: &SqlitePool,
    request: &ReportImportRequest,
) -> Result<(), AppError> {
    let envelope = &request.envelope;
    if envelope.schema_version != "1" || envelope.revision == 0 || envelope.run_id.is_empty() {
        return Err(AppError::validation(
            "invalid_envelope",
            "schema_version 1, run_id, and positive revision are required",
        ));
    }
    if envelope.body_markdown_path != "report.md" {
        return Err(AppError::validation(
            "invalid_body_path",
            "body_markdown_path must be report.md",
        ));
    }
    if let Some(markdown) = envelope.artifacts.get("md")
        && markdown.hash != envelope.body_hash
    {
        return Err(AppError::validation(
            "artifact_hash_mismatch",
            "Markdown artifact hash differs from body_hash",
        ));
    }
    if envelope.artifacts.keys().any(|format| format != "md") {
        return Err(AppError::validation(
            "unattached_artifact",
            "Phase 0 import only accepts the attached Markdown artifact",
        ));
    }
    match (&envelope.provider_native, &request.provider_native) {
        (Some(reference), Some(native))
            if reference.hash == sha256(&serde_json::to_vec(native)?) => {}
        (None, None) => {}
        _ => {
            return Err(AppError::validation(
                "provider_native_hash_mismatch",
                "provider_native attachment and envelope reference must agree",
            ));
        }
    }
    let claim_ids = envelope
        .claims
        .iter()
        .map(|claim| claim.id.as_str())
        .collect::<HashSet<_>>();
    if claim_ids.len() != envelope.claims.len() {
        return Err(AppError::validation(
            "duplicate_claim_id",
            "claim IDs must be unique",
        ));
    }
    let evidence_ids = envelope
        .evidence
        .iter()
        .map(|evidence| evidence.id.as_str())
        .collect::<HashSet<_>>();
    if evidence_ids.len() != envelope.evidence.len() {
        return Err(AppError::validation(
            "duplicate_evidence_id",
            "evidence IDs must be unique",
        ));
    }
    for claim in &envelope.claims {
        if !request.body_markdown.contains(&format!("[^{}]", claim.id)) {
            return Err(AppError::validation(
                "missing_claim_marker",
                format!("Markdown has no marker for {}", claim.id),
            ));
        }
        if claim
            .evidence
            .iter()
            .any(|id| !evidence_ids.contains(id.as_str()))
        {
            return Err(AppError::validation(
                "unknown_evidence",
                format!("claim {} references unknown evidence", claim.id),
            ));
        }
        if matches!(
            claim.kind,
            ClaimKind::Calculation | ClaimKind::Comparison | ClaimKind::Projection
        ) && claim.derivation.is_none()
        {
            return Err(AppError::validation(
                "missing_derivation",
                format!("claim {} requires a derivation", claim.id),
            ));
        }
    }
    let listed_extractions = envelope
        .sources
        .iter()
        .map(|source| source.extraction.as_str())
        .collect::<HashSet<_>>();
    if envelope
        .evidence
        .iter()
        .any(|evidence| !listed_extractions.contains(evidence.extraction.as_str()))
    {
        return Err(AppError::validation(
            "unlisted_evidence_source",
            "every evidence extraction must appear in sources",
        ));
    }
    for source in &envelope.sources {
        let exists: i64 = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM extractions e JOIN acquisitions a ON a.id = e.acquisition_id WHERE e.id = ? AND a.id = ?)",
        )
        .bind(&source.extraction)
        .bind(&source.acquisition)
        .fetch_one(pool)
        .await?;
        if exists == 0 {
            return Err(AppError::validation(
                "unknown_source",
                "report source acquisition/extraction does not resolve",
            ));
        }
    }
    Ok(())
}

async fn check_evidence(
    pool: &SqlitePool,
    store: &ArtifactStore,
    envelope: &ReportEnvelope,
) -> Result<Vec<MechanicalCheck>, AppError> {
    let mut checks = Vec::with_capacity(envelope.evidence.len());
    for evidence in &envelope.evidence {
        let path: Option<String> =
            sqlx::query_scalar("SELECT text_path FROM extractions WHERE id = ?")
                .bind(&evidence.extraction)
                .fetch_optional(pool)
                .await?;
        let path = path.ok_or_else(|| {
            AppError::validation(
                "unknown_extraction",
                format!("{} does not resolve", evidence.extraction),
            )
        })?;
        let text = store.read(&path).await?;
        let source = normalise(
            std::str::from_utf8(&text).map_err(|_| {
                AppError::validation("invalid_extraction", "extraction is not UTF-8")
            })?,
            &evidence.normalisation,
        )?;
        let quote = normalise(&evidence.quote, &evidence.normalisation)?;
        checks.push(if !quote.is_empty() && source.contains(&quote) {
            MechanicalCheck::Passed
        } else {
            MechanicalCheck::Failed
        });
    }
    Ok(checks)
}

fn normalise(value: &str, declared: &str) -> Result<String, AppError> {
    let mut result = value.to_owned();
    for operation in declared
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty() && *value != "none")
    {
        result = match operation {
            "whitespace" => result.split_whitespace().collect::<Vec<_>>().join(" "),
            "unicode-nfkc" => result.nfkc().collect(),
            other => {
                return Err(AppError::validation(
                    "unknown_normalisation",
                    format!("unsupported normalisation: {other}"),
                ));
            }
        };
    }
    Ok(result)
}

pub fn compute_label(envelope: &ReportEnvelope) -> (Label, Vec<String>) {
    let reviewed = matches!(
        envelope.review.level,
        ReviewLevel::MaterialClaimsReviewed | ReviewLevel::FullyReviewed
    );
    let problems = envelope
        .claims
        .iter()
        .filter(|claim| {
            claim.material
                && (claim.review.outcome.as_ref().is_some_and(|outcome| {
                    matches!(
                        outcome,
                        AssessmentOutcome::Unsupported
                            | AssessmentOutcome::Contradicted
                            | AssessmentOutcome::Stale
                    )
                }) || claim.evidence.iter().any(|id| {
                    envelope.evidence.iter().any(|evidence| {
                        evidence.id == *id && evidence.relation == EvidenceRelation::Contradicts
                    })
                }))
        })
        .map(|claim| format!("open_material_claim:{}", claim.id))
        .collect::<Vec<_>>();
    if reviewed && !problems.is_empty() {
        return (Label::NeedsReview, problems);
    }
    let mut reasons = Vec::new();
    if !reviewed {
        reasons.push("material_claims_not_reviewed".into());
    }
    for claim in envelope.claims.iter().filter(|claim| claim.material) {
        if !claim.review.checked
            || !matches!(
                claim.review.outcome,
                Some(AssessmentOutcome::Supported | AssessmentOutcome::Qualified)
            )
        {
            reasons.push(format!("material_claim_not_supported:{}", claim.id));
        }
    }
    for (index, question) in envelope.completion.required_questions.iter().enumerate() {
        if !question.answered && question.unanswerable_reason.is_none() {
            reasons.push(format!("required_question_unanswered:{index}"));
        }
    }
    if envelope.completion.hit_limit.is_some() {
        reasons.push("run_hit_limit".into());
    }
    if envelope
        .assumptions
        .derived
        .iter()
        .any(|assumption| !assumption.answered)
    {
        reasons.push("open_clarification".into());
    }
    if reasons.is_empty() {
        (Label::Reviewed, reasons)
    } else {
        (Label::Draft, reasons)
    }
}

fn sha256(bytes: &[u8]) -> String {
    format!("sha256:{}", hex::encode(Sha256::digest(bytes)))
}

fn relation_name(value: &EvidenceRelation) -> &'static str {
    match value {
        EvidenceRelation::Supports => "supports",
        EvidenceRelation::Contradicts => "contradicts",
        EvidenceRelation::Contextualises => "contextualises",
    }
}

fn label_name(value: &Label) -> &'static str {
    match value {
        Label::Draft => "draft",
        Label::NeedsReview => "needs_review",
        Label::Reviewed => "reviewed",
    }
}

fn review_level_name(value: &ReviewLevel) -> &'static str {
    match value {
        ReviewLevel::Structural => "structural",
        ReviewLevel::Mechanical => "mechanical",
        ReviewLevel::MaterialClaimsReviewed => "material_claims_reviewed",
        ReviewLevel::FullyReviewed => "fully_reviewed",
    }
}
