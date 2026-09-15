# Exa Agent account capability probe

Date: 2026-09-15

This was an explicitly authorised paid call through the service's durable spend
reservation path. It was a capability check, not a golden-case quality score.

## Request

- Effort: `minimal`
- Fixed reservation: $0.012
- Prompt class: Pennsylvania homeschool lookup, limited to two official pages;
  no recommendation requested
- Submission path: managed run → fenced heavy-lane worker → Exa Agent

## Observed result

- Terminal status: `completed`
- Stop reason: `schema_satisfied`
- Polls to terminal: completed within approximately 10 seconds
- Grounding: emitted for both answer items
- Citation detail: URL and title only; no quoted passages
- Provider-reported cost: $0.012
- Reconciliation: the $0.012 reservation was reconciled to $0.012
- External task ID was persisted before polling
- Terminal provider output was collected into the content-addressed store

The two returned sources were a Pennsylvania Department of Education home
education page and an Insight PA enrollment page. The result is sufficient to
confirm `citations: urls_only` for this run. It does not prove grounding appears
on every run.

## Still unverified

- Account Zero Data Retention setting and retention behavior
- Cancellation billing semantics
- `max` stop behavior and partial-output retention
- Fixed-effort behavior at every effort level
- Quality against a dated golden reference

No D-003 backend decision can be made from this probe.
