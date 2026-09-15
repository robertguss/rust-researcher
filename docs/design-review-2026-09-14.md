# Design review, September 14, 2026

Review of the [architecture proposal](rust-research-architecture.md) and the
[landscape comparison](research-tool-landscape.md) as they stood before the
revisions made the same day. Kept so the reasoning behind those revisions is not
lost.

## Verdict

The documents described a credible research **control plane**: durable jobs,
honest provenance, isolated parsers, careful recovery semantics. They did not
describe how good research would be produced, measured, or consumed day to day.
The delivery plan built storage, queues, parsers, and exports for two phases
before establishing whether the reports beat the incumbent. For a tool meant to
be part of daily work, that was the wrong order.

## What was right and was kept

- Service as system of record; Hermes, Claude Code, Codex as peers.
- The provenance distinctions: retrieval time versus publication date; cited URL
  versus acquired text; explicit access levels; provider-native output preserved
  beside normalised output; model agreement is not corroboration.
- `blocked` and `unknown` as first-class states; no promise of exactly-once
  external execution.
- Notification decoupled from research.
- Restraint: SQLite, one worker, one parser, no Redis, no vector DB, no
  multi-agent debate, short-lived Docling processes.
- Consistent honesty that nothing had been built, benchmarked, or deployed.

## What changed and why

| Finding                                                                                       | Change                                                                                                     | Where                                                                                                                  |
| --------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- |
| Phases built retrieval, Docling, and arXiv before any managed report existed                  | Delivery reordered around one complete vertical slice to the phone                                         | [Architecture §10](rust-research-architecture.md#10-first-version-scope-and-delivery-order)                            |
| Semantic review was an optional Phase 3 item                                                  | Material-claims review required before a report is labelled Reviewed; Draft/Reviewed label on every format | [Quality protocol §6–7](research-quality-protocol.md#6-review-levels)                                                  |
| Exa Agent chosen on "the account exists"; Phase 0 was one paid call                           | Replaced by a repeated, scored comparison against one official CLI with a decision rule                    | [Evaluation harness §4](evaluation-harness.md#4-the-backend-comparison-replaces-the-single-exa-task)                   |
| No research protocol, only an eight-step outline in the landscape                             | Versioned execution protocol, material-claim rules, review levels, completion block                        | [Quality protocol](research-quality-protocol.md)                                                                       |
| No golden set, rubric, or hard gates                                                          | Defined                                                                                                    | [Evaluation harness](evaluation-harness.md)                                                                            |
| The homeschool report injected Pennsylvania silently; no mechanism to ask or to carry context | Standing user context, recorded applied assumptions, `clarification` policy and blocked reason             | [Quality protocol §2–3](research-quality-protocol.md#2-standing-user-context)                                          |
| Runs modelled as isolated; daily research is iterative                                        | Follow-up runs with inherited brief and evidence                                                           | [Quality protocol §8](research-quality-protocol.md#8-follow-up-runs)                                                   |
| Partial results promised, not modelled; one `status` for everything                           | Run state split into execution, completeness, review, label, artifacts, notification, spend                | [Contracts §1](run-and-report-contracts.md#1-run-state-is-several-fields-not-one-status)                               |
| `resume` could not resolve `unknown` without risking a duplicate charge                       | Explicit `reconcile` operation with adopt, mark-failed, resubmit                                           | [Contracts §1](run-and-report-contracts.md#unknown-is-resolved-by-reconciliation-not-by-resume)                        |
| "Service enforces limits" versus backends that cannot                                         | Backend capability contract; unsupported hard limits rejected unless explicitly accepted                   | [Contracts §2](run-and-report-contracts.md#2-backend-capability-contract)                                              |
| `--max-cost` and `--max-duration` had no semantics                                            | Per-run, daily, monthly caps; queue time excluded; reserved tail; tool operations count; unknown ≠ zero    | [Contracts §3](run-and-report-contracts.md#3-budget-semantics)                                                         |
| One worker could let OCR starve cancellation and polling                                      | Control and heavy lanes                                                                                    | [Contracts §4](run-and-report-contracts.md#4-worker-lanes)                                                             |
| Bespoke block/table document schema for `report.json`                                         | Small envelope around one Markdown body with claim anchors; structured blocks optional                     | [Contracts §5](run-and-report-contracts.md#5-report-envelope-version-1)                                                |
| API table missing operations the workflows implied                                            | Added; client-led lifecycle replaced by report import in v1                                                | [Contracts §6](run-and-report-contracts.md#6-api-additions)                                                            |
| Web-page acquisition unspecified; landscape's fetch ladder dropped                            | Acquisition ladder, freshness classes, `full_text` ≠ correct                                               | [Acquisition policy](source-acquisition-policy.md)                                                                     |
| "Canonical URL" with no normalisation, dedup, or independence rules                           | URL normalisation, scholarly identity, origin groups, cache keys                                           | [Acquisition policy §3–4](source-acquisition-policy.md#3-source-identity-deduplication-independence)                   |
| Robots, terms, retention not addressed                                                        | Defined                                                                                                    | [Acquisition policy §5](source-acquisition-policy.md#5-terms-robots-and-retention)                                     |
| "Dedicated workspace" as the CLI worker's isolation                                           | OS user, minimal HOME, allow-listed environment, pinned version, canary, failure tests                     | [Runbook §3](operations-runbook.md#3-agent-cli-worker-isolation)                                                       |
| Backup was "test restoring it"                                                                | RPO/RTO, consistent snapshot, GC pause, off-VM encryption, restore rehearsal                               | [Runbook §4](operations-runbook.md#4-backup-restore-and-what-recovery-means)                                           |
| Telegram sent a job ID; reading required SSH plus bearer                                      | Read-only report page behind exe.dev's private proxy; tested tap-to-read on the phone                      | [Runbook §5](operations-runbook.md#5-phone-workflow)                                                                   |
| Observability was structured logs                                                             | One-screen operator view; external heartbeat                                                               | [Runbook §6](operations-runbook.md#6-operator-view)                                                                    |
| Schema evolution and testing strategy absent                                                  | Defined                                                                                                    | [Contracts §7](run-and-report-contracts.md#7-schema-evolution), [Runbook §8](operations-runbook.md#8-testing-strategy) |
| Subscription-first became hosted-first without being called a product decision                | Called out explicitly for approval                                                                         | [Architecture §1](rust-research-architecture.md#1-decision-summary)                                                    |
| Rust chosen without stated reason                                                             | Stated as a maintainer preference, revisable                                                               | [Architecture §1](rust-research-architecture.md#1-decision-summary)                                                    |

## Assumptions that remain unvalidated

Acknowledged in the documents, still unproven, and load-bearing:

| Assumption                                                                        | Validation                                                                                   |
| --------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- |
| A hosted backend can produce defensible, evidence-accessible research             | Evaluation harness backend comparison                                                        |
| Subscriptions can absorb unattended research without wrecking coding quota        | Same comparison, with quota events recorded                                                  |
| The exe.dev VM can run parsing, rendering, and an agent within the latency target | Measurements on the actual VM, in Phase 0                                                    |
| Public sources are fresh and complete enough through provider contents            | Trap and market cases in the golden set                                                      |
| Hosted output normalises into the envelope without losing meaning                 | Round-trip real outputs and compare by hand                                                  |
| It is cheaper than the incumbent                                                  | Pilot accounting including review time and maintenance                                       |
| Five to ten minutes end to end is achievable                                      | Measured from submission to final artifact on repeated cases                                 |
| Rust is the right coordinator language for a solo maintainer                      | Robert's own judgement; revisit if iteration on prompts, adapters, and evaluation feels slow |
| arXiv and Docling belong in an early phase                                        | Only if the golden set's paper-heavy cases need them                                         |

## Independent review, adopted the same day

A second reviewer assessed the original documents in parallel, without seeing
this review. The two reviews agreed on the verdict (reliability design sound,
delivery order and quality gating wrong, Exa chosen on convenience) and on most
of the missing items. The independent review added things this one had missed,
and they were adopted into the companion documents rather than argued with.

Adopted from the independent review:

- A **ranked product objective** to break ties, now in
  [architecture §1](rust-research-architecture.md#1-decision-summary) for Robert
  to confirm.
- **Privacy as its own policy**: classification, provider routing, minimisation,
  key custody, deletion semantics, and personal/work scopes
  ([runbook §9](operations-runbook.md#9-privacy-classification-and-outbound-policy)).
  The original documents protected sources and credentials and said nothing
  about what the brief itself reveals.
- **Capability contract fields** for partial output, result retention,
  submission idempotency, and privacy mode, bound to the API version and account
  settings, with the Exa facts checked against the current guide
  ([contracts §2](run-and-report-contracts.md#2-backend-capability-contract)).
  The ZDR finding matters most: under zero data retention a worker outage during
  collection loses the run even when the task ID was persisted.
- **Transactional publication** and **notification uniqueness**
  ([architecture §6](rust-research-architecture.md#6-job-lifecycle-and-recovery)
  rules 8 and 9). This review's version protected the file and the row; it did
  not protect the notification intent.
- **Attempt-epoch fencing on every write**, not only on completion (rule 10).
- **Reconciliation mode after restore** (rule 11 and
  [runbook §4](operations-runbook.md#4-backup-restore-and-what-recovery-means)).
- **`source_versions` split into acquisitions and extractions** with
  `content_kind`, a **searches** record including null results, and evidence
  **relations** and claim **derivations**
  ([architecture §4](rust-research-architecture.md#4-data-model),
  [acquisition policy §1](source-acquisition-policy.md#1-acquisition-ladder)).
- **Append-only assessments** with a policy version, a third label
  `needs_review`, and **corrections as new revisions**.
- **Archive operations**: find, paged evidence inspection, correct, bundle
  export; **distinct `rerender`/`recheck`/`refresh`** so an agent never guesses
  what `resume` will do; **truncation with continuation cursors**
  ([contracts §6](run-and-report-contracts.md#6-api-additions)).
- **Child processes with no network and no credentials** and **manifest
  validation** before publication.
- The **mixed-load VM test** in Phase 0, and the reviewer's **deterministic test
  table** merged into [runbook §8](operations-runbook.md#8-testing-strategy).
- Four more **golden-set categories** (historical as-of, conflicting sources,
  inaccessible source, location-ambiguous) and the **rework-time** metric.
- **Decision records** in [decisions.md](decisions.md), replacing "researched
  earlier in this conversation" as a source of truth.

Not adopted as proposed, with the compromise recorded:

- **Client-led loop first.** The independent review would build the full
  client-led lifecycle (create run, attach evidence, submit) before any managed
  execution. This review keeps one-shot import
  ([decision D-007](decisions.md#d-007-one-shot-report-import-instead-of-incremental-client-led-attach))
  but moves a **thin evidence loop** (search, contents, store, validator,
  import) into Phase 0 as evaluation infrastructure
  ([D-008](decisions.md#d-008-phase-0-builds-the-thin-evidence-loop)). The
  reviewer's aim, testing the service's unique value before debugging hosted
  autonomy, is met without the half-finished-run state model.

Verified while adopting: Exa's `grounding` is "when emitted";
`budget.maxCostDollars` applies only to `auto` and `max` effort; ZDR deletes
uncollected results immediately and disables `previousRunId`. Not verified:
whether stopping a task retains partial output, and for which efforts. That is a
Phase 0 item.

## Reviewer's note on method

The review was performed against the documents, the two public exe.dev pages
linked from the runbook, and the vendor pages already cited in the landscape. No
provider was called, no VM was inspected, and vendor prices were not re-audited.
Everything in the new documents that depends on the target VM or a provider is
marked as needing verification.
