ALTER TABLE runs ADD COLUMN backend_config_json TEXT NOT NULL DEFAULT '{}';

CREATE TABLE provider_runs (
    id TEXT PRIMARY KEY,
    run_id TEXT NOT NULL REFERENCES runs(id),
    job_id TEXT NOT NULL REFERENCES jobs(id),
    provider TEXT NOT NULL,
    external_task_id TEXT NOT NULL,
    status TEXT NOT NULL,
    stop_reason TEXT,
    output_hash TEXT NOT NULL,
    output_path TEXT NOT NULL,
    reported_microusd INTEGER,
    collected_at TEXT NOT NULL,
    UNIQUE(provider, external_task_id)
);
