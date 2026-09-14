# Rust Researcher

An API-first research application with a thin CLI, designed for use by Hermes,
Claude Code, Codex, and other agents or harnesses.

The service will own research jobs, sources, evidence, public PDF parsing,
reports, exports, and Telegram notifications. Investigations can be delegated to
external agents or hosted research APIs.

## Status

Design and research only. No application has been implemented, deployed, or
benchmarked yet.

## Documents

- **[Rust architecture proposal](docs/rust-research-architecture.md)** — the
  current proposed direction: API/CLI boundaries, storage, job lifecycle, worker
  isolation, PDF processing, and phased delivery.
- **[Research tool landscape](docs/research-tool-landscape.md)** — supporting
  comparisons of open-source projects, search APIs, academic retrieval, and
  hosted research services. Its earlier CLI-first recommendation is superseded
  by the API-first architecture proposal.

## Proposed stack

- Rust with Axum and Tokio for the API and worker.
- SQLite and SQLx for persistent records.
- A thin Rust CLI sharing protocol types with the service.
- Isolated Python/Docling subprocesses for rich PDF parsing.
- Exa retrieval initially, with replaceable research backends.
- Versioned JSON reports with Markdown and PDF exports.

The next proposed step is a small feasibility pass on backend output quality,
PDF parsing resource usage, durable queue recovery, and report rendering before
building the application.
