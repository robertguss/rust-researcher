CREATE TABLE report_artifacts (
    report_id TEXT NOT NULL REFERENCES reports(id),
    format TEXT NOT NULL CHECK(format IN ('html','pdf')),
    content_hash TEXT NOT NULL,
    content_path TEXT NOT NULL,
    created_at TEXT NOT NULL,
    PRIMARY KEY(report_id, format)
);
