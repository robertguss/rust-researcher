CREATE TABLE sources (
    id TEXT PRIMARY KEY,
    canonical_url TEXT NOT NULL,
    aliases_json TEXT NOT NULL DEFAULT '[]',
    title TEXT,
    origin_group TEXT NOT NULL,
    freshness_class TEXT NOT NULL,
    created_at TEXT NOT NULL,
    UNIQUE(canonical_url)
);

CREATE TABLE acquisitions (
    id TEXT PRIMARY KEY,
    source_id TEXT NOT NULL REFERENCES sources(id),
    retrieved_at TEXT NOT NULL,
    final_url TEXT NOT NULL,
    http_status INTEGER,
    headers_json TEXT NOT NULL DEFAULT '{}',
    raw_hash TEXT NOT NULL,
    raw_path TEXT NOT NULL,
    content_kind TEXT NOT NULL CHECK(content_kind IN ('raw','provider_extract','model_summary')),
    access_level TEXT NOT NULL CHECK(access_level IN ('metadata_only','snippet','partial_text','full_text')),
    acquisition_rung INTEGER NOT NULL,
    failure_reason TEXT
);

CREATE TABLE extractions (
    id TEXT PRIMARY KEY,
    acquisition_id TEXT NOT NULL REFERENCES acquisitions(id),
    extractor TEXT NOT NULL,
    extraction_version TEXT NOT NULL,
    options_json TEXT NOT NULL DEFAULT '{}',
    text_hash TEXT NOT NULL,
    text_path TEXT NOT NULL,
    extraction_confidence REAL,
    extraction_warnings_json TEXT NOT NULL DEFAULT '[]',
    created_at TEXT NOT NULL
);

CREATE TABLE searches (
    id TEXT PRIMARY KEY,
    run_id TEXT,
    backend TEXT NOT NULL,
    query TEXT NOT NULL,
    searched_at TEXT NOT NULL,
    result_count INTEGER NOT NULL,
    results_json TEXT NOT NULL
);

CREATE TABLE evidence (
    id TEXT PRIMARY KEY,
    run_id TEXT NOT NULL,
    extraction_id TEXT NOT NULL REFERENCES extractions(id),
    relation TEXT NOT NULL CHECK(relation IN ('supports','contradicts','contextualises')),
    locator_json TEXT NOT NULL,
    quoted_text TEXT NOT NULL,
    normalisation TEXT NOT NULL,
    mechanical_check TEXT NOT NULL CHECK(mechanical_check IN ('passed','failed'))
);

CREATE TABLE reports (
    id TEXT PRIMARY KEY,
    run_id TEXT NOT NULL,
    revision INTEGER NOT NULL,
    supersedes INTEGER,
    envelope_hash TEXT NOT NULL,
    envelope_path TEXT NOT NULL,
    body_hash TEXT NOT NULL,
    body_path TEXT NOT NULL,
    current_computed_label TEXT NOT NULL CHECK(current_computed_label IN ('draft','needs_review','reviewed')),
    review_level TEXT NOT NULL,
    artifact_manifest_json TEXT NOT NULL,
    created_at TEXT NOT NULL,
    UNIQUE(run_id, revision)
);

CREATE TABLE assessments (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    report_id TEXT NOT NULL REFERENCES reports(id),
    assessed_at TEXT NOT NULL,
    reviewer TEXT NOT NULL,
    policy_version TEXT NOT NULL,
    computed_label TEXT NOT NULL CHECK(computed_label IN ('draft','needs_review','reviewed')),
    reasons_json TEXT NOT NULL
);

CREATE TABLE spend (
    id TEXT PRIMARY KEY,
    operation_kind TEXT NOT NULL,
    operation_id TEXT NOT NULL,
    provider TEXT NOT NULL,
    reserved_microusd INTEGER NOT NULL,
    reported_microusd INTEGER,
    state TEXT NOT NULL CHECK(state IN ('reserved','reconciled','released','unknown')),
    idempotency_key TEXT NOT NULL UNIQUE,
    created_at TEXT NOT NULL,
    reconciled_at TEXT
);

CREATE TABLE events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    run_id TEXT,
    event_type TEXT NOT NULL,
    occurred_at TEXT NOT NULL,
    payload_json TEXT NOT NULL
);
