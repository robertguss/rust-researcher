CREATE TABLE runs (
    id TEXT PRIMARY KEY,
    brief_json TEXT NOT NULL,
    mode TEXT NOT NULL CHECK(mode IN ('managed','imported')),
    backend TEXT NOT NULL,
    backend_contract_version TEXT NOT NULL,
    depth TEXT NOT NULL CHECK(depth IN ('lookup','standard','deep','extended')),
    max_duration_seconds INTEGER,
    max_cost_microusd INTEGER,
    accept_weaker_limits INTEGER NOT NULL DEFAULT 0,
    follow_up_of TEXT REFERENCES runs(id),
    context_version INTEGER,
    instruction_version TEXT NOT NULL,
    execution TEXT NOT NULL CHECK(execution IN ('queued','running','succeeded','failed','blocked','unknown','cancelled')),
    blocked_reason TEXT CHECK(blocked_reason IN ('reauth','spend_decision','quota','clarification','reconcile','disk') OR blocked_reason IS NULL),
    completeness TEXT NOT NULL CHECK(completeness IN ('none','partial','complete')),
    review TEXT NOT NULL CHECK(review IN ('structural','mechanical','material_claims_reviewed','fully_reviewed')),
    label TEXT NOT NULL CHECK(label IN ('draft','needs_review','reviewed')),
    notification TEXT NOT NULL CHECK(notification IN ('pending','delivered','failed','unknown')),
    external TEXT NOT NULL CHECK(external IN ('none','submitted','accepted','running','terminal','unreconciled')),
    current_report_id TEXT REFERENCES reports(id),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE jobs (
    id TEXT PRIMARY KEY,
    run_id TEXT REFERENCES runs(id),
    source_id TEXT REFERENCES sources(id),
    task_kind TEXT NOT NULL CHECK(task_kind IN ('research','fetch_parse','export','notification')),
    lane TEXT NOT NULL CHECK(lane IN ('control','heavy')),
    state TEXT NOT NULL CHECK(state IN ('queued','running','succeeded','failed','blocked','unknown','cancelled','retry_wait')),
    attempt INTEGER NOT NULL DEFAULT 0,
    attempt_epoch INTEGER NOT NULL DEFAULT 0,
    lease_owner TEXT,
    lease_expires_at TEXT,
    retry_at TEXT,
    external_task_id TEXT,
    idempotency_key TEXT NOT NULL UNIQUE,
    error_json TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    CHECK((run_id IS NOT NULL) != (source_id IS NOT NULL))
);

CREATE TABLE idempotency_keys (
    key TEXT PRIMARY KEY,
    request_hash TEXT NOT NULL,
    run_id TEXT NOT NULL REFERENCES runs(id),
    created_at TEXT NOT NULL
);

CREATE TABLE run_sources (
    run_id TEXT NOT NULL REFERENCES runs(id),
    acquisition_id TEXT NOT NULL REFERENCES acquisitions(id),
    search_id TEXT REFERENCES searches(id),
    provenance TEXT NOT NULL,
    inclusion_reason TEXT,
    exclusion_reason TEXT,
    inherited_from TEXT REFERENCES runs(id),
    PRIMARY KEY(run_id, acquisition_id)
);

CREATE TABLE context (
    scope TEXT NOT NULL CHECK(scope IN ('personal','work')),
    version INTEGER NOT NULL,
    document_json TEXT NOT NULL,
    created_at TEXT NOT NULL,
    PRIMARY KEY(scope, version)
);

CREATE TABLE corrections (
    id TEXT PRIMARY KEY,
    report_id TEXT NOT NULL REFERENCES reports(id),
    new_report_id TEXT NOT NULL REFERENCES reports(id),
    claim_ids_json TEXT NOT NULL,
    reason TEXT NOT NULL,
    corrected_by TEXT NOT NULL,
    corrected_at TEXT NOT NULL
);

CREATE TABLE notifications (
    id TEXT PRIMARY KEY,
    run_id TEXT NOT NULL REFERENCES runs(id),
    report_id TEXT REFERENCES reports(id),
    revision INTEGER,
    kind TEXT NOT NULL,
    channel TEXT NOT NULL,
    state TEXT NOT NULL CHECK(state IN ('pending','delivered','failed','unknown')),
    attempts INTEGER NOT NULL DEFAULT 0,
    payload_json TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    UNIQUE(run_id, revision, kind)
);

CREATE TRIGGER assessments_no_update
BEFORE UPDATE ON assessments BEGIN SELECT RAISE(ABORT, 'assessments are append-only'); END;

CREATE TRIGGER assessments_no_delete
BEFORE DELETE ON assessments BEGIN SELECT RAISE(ABORT, 'assessments are append-only'); END;
