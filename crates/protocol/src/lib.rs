use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchRequest {
    pub query: String,
    #[serde(default = "default_result_count")]
    pub result_count: u32,
}

fn default_result_count() -> u32 {
    10
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub url: String,
    pub title: Option<String>,
    pub published_date: Option<String>,
    pub exclusion_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResponse {
    pub id: String,
    pub backend: String,
    pub query: String,
    pub at: DateTime<Utc>,
    pub result_count: u32,
    pub results: Vec<SearchResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceRequest {
    pub url: String,
    #[serde(default)]
    pub freshness_class: FreshnessClass,
    #[serde(default)]
    pub require_live: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FreshnessClass {
    Volatile,
    Current,
    #[default]
    Stable,
    Immutable,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceResponse {
    pub source_id: String,
    pub acquisition_id: String,
    pub extraction_id: String,
    pub canonical_url: String,
    pub final_url: String,
    pub content_kind: ContentKind,
    pub access_level: AccessLevel,
    pub acquisition_rung: u8,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ContentKind {
    Raw,
    ProviderExtract,
    ModelSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AccessLevel {
    MetadataOnly,
    Snippet,
    PartialText,
    FullText,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReportImportRequest {
    pub envelope: ReportEnvelope,
    pub body_markdown: String,
    pub provider_native: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReportImportResponse {
    pub report_id: String,
    pub run_id: String,
    pub revision: u32,
    pub label: Label,
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReportEnvelope {
    pub schema_version: String,
    pub run_id: String,
    pub revision: u32,
    pub supersedes: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<Label>,
    pub produced_by: ProducedBy,
    pub brief: Value,
    pub assumptions: Assumptions,
    pub body_markdown_path: String,
    pub body_hash: String,
    pub claims: Vec<Claim>,
    pub evidence: Vec<Evidence>,
    #[serde(default)]
    pub assessments: Vec<Assessment>,
    pub sources: Vec<ReportSource>,
    #[serde(default)]
    pub searches: Vec<ReportSearch>,
    pub completion: Completion,
    pub review: Review,
    pub provider_native: Option<ArtifactRef>,
    #[serde(default)]
    pub artifacts: BTreeMap<String, ArtifactRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProducedBy {
    pub backend: String,
    #[serde(default)]
    pub backend_config: Value,
    pub instruction_version: String,
    pub context_version: u32,
    pub model: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Assumptions {
    #[serde(default)]
    pub from_brief: Vec<Value>,
    #[serde(default)]
    pub from_context: Vec<Value>,
    #[serde(default)]
    pub derived: Vec<DerivedAssumption>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DerivedAssumption {
    pub question: String,
    pub assumed: String,
    pub answered: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claim {
    pub id: String,
    pub kind: ClaimKind,
    pub text: String,
    pub material: bool,
    #[serde(default)]
    pub evidence: Vec<String>,
    pub derivation: Option<Derivation>,
    pub review: ClaimReview,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ClaimKind {
    Observation,
    Calculation,
    Inference,
    Recommendation,
    Comparison,
    Projection,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Derivation {
    pub inputs: Vec<String>,
    pub method: String,
    pub as_of: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaimReview {
    pub checked: bool,
    pub by: Option<String>,
    pub note: Option<String>,
    #[serde(default)]
    pub outcome: Option<AssessmentOutcome>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Evidence {
    pub id: String,
    pub extraction: String,
    pub relation: EvidenceRelation,
    pub locator: Value,
    pub quote: String,
    pub normalisation: String,
    #[serde(default)]
    pub mechanical_check: Option<MechanicalCheck>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceRelation {
    Supports,
    Contradicts,
    Contextualises,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MechanicalCheck {
    Passed,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Assessment {
    pub at: DateTime<Utc>,
    pub by: String,
    pub policy_version: String,
    pub label: Label,
    #[serde(default)]
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReportSource {
    pub acquisition: String,
    pub extraction: String,
    pub content_kind: ContentKind,
    pub access_level: AccessLevel,
    pub retrieved_at: DateTime<Utc>,
    pub origin_group: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReportSearch {
    pub id: String,
    pub backend: String,
    pub query: String,
    pub at: DateTime<Utc>,
    pub result_count: u32,
    #[serde(default)]
    pub returned_urls: Vec<String>,
    #[serde(default)]
    pub excluded: Vec<ExcludedUrl>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExcludedUrl {
    pub url: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Completion {
    pub required_questions: Vec<RequiredQuestion>,
    pub hit_limit: Option<String>,
    pub what_would_change_this: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequiredQuestion {
    pub q: String,
    pub answered: bool,
    #[serde(default)]
    pub unanswerable_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Review {
    pub level: ReviewLevel,
    #[serde(default)]
    pub records: Vec<ReviewRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReviewLevel {
    Structural,
    Mechanical,
    MaterialClaimsReviewed,
    FullyReviewed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewRecord {
    pub by: String,
    #[serde(default)]
    pub checked: Vec<String>,
    pub at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AssessmentOutcome {
    Supported,
    Qualified,
    Unsupported,
    Contradicted,
    Stale,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Label {
    Draft,
    NeedsReview,
    Reviewed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactRef {
    pub path: String,
    pub hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackendContract {
    pub name: String,
    pub adapter_version: String,
    pub capabilities: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SummaryResponse {
    pub spend_today_usd: String,
    pub daily_cap_usd: String,
    pub spend_month_usd: String,
    pub monthly_cap_usd: String,
    pub unknown_spend_count: u64,
    pub reports: u64,
    pub sources: u64,
    pub searches: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateRunRequest {
    pub brief: ResearchBrief,
    pub backend: String,
    #[serde(default)]
    pub backend_config: Value,
    pub max_duration_seconds: Option<u64>,
    pub max_cost_usd: Option<String>,
    #[serde(default)]
    pub accept_weaker_limits: bool,
    pub follow_up_of: Option<String>,
    pub classification_override_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResearchBrief {
    pub question: String,
    #[serde(default = "unknown_decision")]
    pub decision: String,
    pub audience: Option<String>,
    pub locale: Option<String>,
    pub as_of: DateTime<Utc>,
    #[serde(default)]
    pub required_questions: Vec<String>,
    #[serde(default)]
    pub constraints: Value,
    #[serde(default)]
    pub exclusions: Value,
    #[serde(default)]
    pub assumptions: Value,
    pub depth: Depth,
    #[serde(default)]
    pub clarification: Clarification,
    #[serde(default)]
    pub scope: Scope,
    pub classification: Option<Classification>,
    #[serde(default)]
    pub evidence_policy: Value,
    #[serde(default)]
    pub output: Value,
}

fn unknown_decision() -> String {
    "unknown".into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Depth {
    Lookup,
    Standard,
    Deep,
    Extended,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Clarification {
    Ask,
    #[default]
    Assume,
    Hold,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    #[default]
    Personal,
    Work,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Classification {
    Public,
    PersonalSensitive,
    WorkConfidential,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunResponse {
    pub id: String,
    pub brief: ResearchBrief,
    pub mode: String,
    pub backend: String,
    pub backend_contract_version: String,
    pub execution: String,
    pub blocked_reason: Option<String>,
    pub completeness: String,
    pub review: String,
    pub label: Label,
    pub notification: String,
    pub external: String,
    pub current_report_id: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderRunResponse {
    pub run_id: String,
    pub provider: String,
    pub external_task_id: String,
    pub status: String,
    pub stop_reason: Option<String>,
    pub reported_cost_usd: Option<String>,
    pub collected_at: DateTime<Utc>,
    pub output: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum ReconcileRequest {
    Adopt { external_task_id: String },
    MarkFailed,
    Resubmit { accept_charge: bool },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextDocument {
    pub scope: Scope,
    pub version: u32,
    pub document: Value,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetContextRequest {
    pub document: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorResponse {
    pub code: String,
    pub message: String,
    pub retryable: bool,
    pub request_id: String,
}
