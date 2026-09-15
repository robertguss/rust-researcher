# Decision records

One entry per decision that would be expensive to reverse or that a future
reader will otherwise re-argue from scratch. Each entry names the decision, the
alternatives that were live, the evidence used, what would reverse it, and its
status. "Conversation-dependent" reasoning does not belong in an architecture
document; it belongs here, dated.

Status values: `proposed` (awaiting evidence or Robert), `accepted`,
`superseded`, `rejected`.

On September 14, 2026 Robert delegated the open judgement calls to the reviewing
agent ("make the decisions based upon what you would do"). Entries marked
_accepted by delegation_ were decided under that instruction; Robert can
overturn any of them by editing the entry.

---

## D-001: Ranked product objective

- **Date:** 2026-09-14
- **Status:** accepted by delegation
- **Decision:** Trustworthy answers > reliable completion > agent integration
  and durable archive > cost control > presentation
  ([architecture §1](rust-research-architecture.md#1-decision-summary)).
- **Alternatives:** Presentation first (match the Perplexity report's look);
  cost first (minimise spend by reusing subscriptions).
- **Evidence:** The Perplexity report is the presentation benchmark, but the
  reason to build is that its claims cannot be inspected. Every later trade-off
  in the documents assumes this order.
- **Reversed by:** Robert reordering it. If cost moves above reliable
  completion, the hosted-first route and the reservation-before-submission rule
  both need rethinking.

## D-002: Rust as the coordinator language

- **Date:** 2026-09-14
- **Status:** accepted (Robert's preference, confirmed by delegation)
- **Decision:** Rust with Axum, Tokio, SQLx for the API, worker, and CLI.
- **Alternatives:** Python (faster iteration on prompts and adapters, same
  ecosystem as Docling); TypeScript.
- **Evidence:** Maintainer preference. No technical requirement forces Rust; the
  heavy work is Python, a browser, and an agent CLI regardless.
- **Reversed by:** Phase 0 or Phase 1 showing that adapter and prompt iteration
  is materially slower than the pilot can tolerate. The fallback is a Python
  coordinator with the same contracts.

## D-003: Hosted-first execution route

- **Date:** 2026-09-14
- **Status:** proposed, awaiting the backend comparison
- **Decision:** The first unattended research backend is whichever candidate
  wins the
  [backend comparison](evaluation-harness.md#4-the-backend-comparison-replaces-the-single-exa-task);
  Exa Agent is the first candidate because the account exists and the API is
  asynchronous, not because it has been shown to be good.
- **Alternatives:** Official CLI under Robert's subscriptions first (the
  landscape document's option A); Parallel or You.com.
- **Evidence:** None yet. This reverses the landscape document's
  subscription-first recommendation and its stated requirement that existing
  subscriptions provide the reasoning.
- **Reversed by:** The hosted candidate failing a hard gate on more than one
  repeated case, or its output lacking the citation detail the service needs to
  acquire and verify sources. Either moves the official-CLI worker to Phase 1.

## D-004: SQLite, not PostgreSQL

- **Date:** 2026-09-14
- **Status:** accepted
- **Decision:** SQLite in WAL mode on the VM's local disk, through SQLx.
- **Alternatives:** PostgreSQL on the same VM; a managed database.
- **Evidence:** Single user, one VM, one worker process, write volume of a few
  rows per second at most. Consistent backup is `VACUUM INTO` plus the immutable
  store.
- **Reversed by:** A second VM, or measured write contention between the API and
  worker that busy timeouts do not resolve.

## D-005: Queue implementation

- **Date:** 2026-09-14
- **Status:** proposed, awaiting the Phase 0 test
- **Decision:** Evaluate pinned Apalis with SQLite against the first slice's
  recovery requirements; fall back to a narrow SQLx lease coordinator.
- **Alternatives:** Redis-backed queue; a custom executor from the start.
- **Evidence:** None yet.
- **Reversed by:** Apalis requiring a second lifecycle state machine to express
  `blocked`, `unknown`, and attempt-epoch fencing.

## D-006: Two lanes in one worker process

- **Date:** 2026-09-14
- **Status:** accepted
- **Decision:** Control lane (polling, cancellation, notifications, heartbeats)
  and heavy lane (research, parsing, rendering) as two bounded task sets in one
  process.
- **Alternatives:** Two processes; one undifferentiated pool.
- **Evidence:** A single OCR job must not delay a cancellation; two processes
  add a coordination problem the single user does not need.
- **Reversed by:** The Phase 0 mixed-load test showing the control lane's
  latency cannot be protected inside one process under memory pressure.

## D-007: One-shot report import instead of incremental client-led attach

- **Date:** 2026-09-14
- **Status:** accepted for v1
- **Decision:** An agent that does its own research submits a finished envelope
  to `POST /v1/reports/import`. Incremental evidence attachment to an open run
  is deferred.
- **Alternatives:** The original client-led lifecycle (create run, attach
  evidence, submit report); the independent review's proposal to build the
  client-led loop first.
- **Evidence:** Import gives the evaluation harness and Claude Code the same
  evidence rules as managed runs with one endpoint. Incremental attach needs a
  half-finished-run state model nobody has asked for yet.
- **Reversed by:** A demonstrated need to hand an unfinished investigation
  between harnesses.

## D-008: Phase 0 builds the thin evidence loop

- **Date:** 2026-09-14
- **Status:** accepted by delegation
- **Decision:** Phase 0 includes Exa search/contents endpoints, the acquisition
  and extraction store, the envelope validator, and report import, so the
  official-CLI candidate can be scored on the same evidence rules as the hosted
  one and Claude Code can use the store immediately.
- **Alternatives:** Pure evaluation with no code (the earlier revision of the
  plan); the independent review's full client-led loop first.
- **Evidence:** The comparison cannot be fair without it; a CLI candidate
  without an evidence store has nowhere to put quotes.
- **Reversed by:** Nothing expected; it is a subset of Phase 1 either way.

## D-009: Label computed by the service, three values, append-only assessments

- **Date:** 2026-09-14
- **Status:** accepted
- **Decision:** `draft`, `needs_review`, `reviewed`, computed from completion,
  review, and open assumptions; every computation appends an assessment with the
  policy version. No "verified" badge.
- **Alternatives:** Worker-supplied label; a single boolean; a verified tier.
- **Evidence:** A worker grading its own work is the failure mode the labels
  exist to prevent. "Verified" promises what a single-user system cannot
  deliver.
- **Reversed by:** Nothing expected.

## D-010: Classification and outbound routing before professional use

- **Date:** 2026-09-14
- **Status:** accepted by delegation
- **Decision:** Every brief carries a classification; each backend declares
  which it accepts; rejection happens before transmission
  ([runbook §9](operations-runbook.md#9-privacy-classification-and-outbound-policy)).
  Two contested cells were settled: **Exa Agent never receives a
  `work_confidential` brief** except by per-run override with a recorded reason,
  because the whole brief leaves Robert's control and Exa's retention behaviour
  under ZDR is unverified; and **`work` scope defaults to `work_confidential`**,
  so an agent must say `public` to use the hosted route on work. The friction is
  the point: a wrongly-public default leaks once and is unrecoverable; a
  wrongly-confidential default costs one retry.
- **Alternatives:** Trust the operator to not submit confidential briefs; a
  single "private" flag.
- **Evidence:** Public sources do not make a query public; the independent
  review made this point and it was missing from every document.
- **Reversed by:** Nothing expected. The routing table itself will change.

## D-011: `source_versions` split into acquisitions and extractions

- **Date:** 2026-09-14
- **Status:** accepted
- **Decision:** Acquisitions record what was fetched and its `content_kind`;
  extractions record how it was read. Evidence binds to an extraction.
- **Alternatives:** One `source_versions` record with extraction fields.
- **Evidence:** Re-extracting must not look like the page changed; a provider's
  extract must not be mistaken for the page.
- **Reversed by:** Nothing expected.

## D-012: Backup destination and key custody

- **Date:** 2026-09-14
- **Status:** accepted (destination chosen by Robert; mechanism by delegation)
- **Decision:** restic to a **Cloudflare R2** bucket over the S3 API, hourly.
  Repository password generated once, stored in Robert's password manager and on
  one printed copy; never in the Cloudflare account. Secrets directory backed up
  separately as a dated age-encrypted file to the same bucket. The VM's token is
  Object Read & Write scoped to the one bucket; deletion is prevented by an R2
  **bucket lock** (30 days on `data/`, `snapshots/`, `keys/`, `secrets/`;
  indefinite on `config`; `locks/` and `index/` unlocked because restic rewrites
  them). `forget --prune` runs monthly from Robert's machine.
- **Alternatives:** Backblaze B2 (the first choice; it offers
  write-without-delete application keys, which R2 does not); rclone plus age to
  S3 (two tools for what restic does in one); a home NAS (single site, off when
  the house loses power); a second VM at another provider.
- **Evidence:** Robert's preference for R2, stated September 14, 2026. R2 is
  S3-compatible, charges no egress, and is not exe.dev. Cloudflare's
  [token documentation](https://developers.cloudflare.com/r2/api/tokens) shows
  only Read-only and Read & Write object permissions, so a key cannot be made
  write-only; its
  [bucket-lock documentation](https://developers.cloudflare.com/r2/buckets/bucket-locks)
  shows prefix rules that block deletion and overwriting for a period and take
  precedence over lifecycle rules, which gives the same protection at the bucket
  instead of the key. restic gives client-side encryption, deduplication, and
  retention in one pinned binary.
- **Known constraint:** prune must be able to delete packs it makes obsolete;
  packs younger than the lock window cannot be deleted. Monthly prune with a
  30-day lock is expected to work because obsolete packs are almost always older
  than that, but this is **verified in the first restore rehearsal**, not
  assumed. If it fails, the fix is a longer prune interval, not a shorter lock.
- **Reversed by:** The rehearsal showing restic and bucket locks cannot coexist
  even with the prefix exclusions; then B2 with a write-only key is the fallback
  and the runbook changes destination, not procedure.
- **Setup task:** create the bucket, the bucket-lock rules, and the scoped
  token; put the restic password in the manager; print it. Recorded in the
  runbook as **verify** until done.

## D-013: Official CLI candidate for the comparison

- **Date:** 2026-09-14
- **Status:** accepted by delegation
- **Decision:** Claude Code is the official-CLI arm of the Phase 0 comparison.
  If it produces quota or rate-limit events that affect Robert's interactive use
  during the round, the three repeated cases are also run on Codex and the
  observation is recorded.
- **Alternatives:** Codex; both from the start (doubles the CLI arm's cost and
  scoring time for a decision that only needs one survivor).
- **Evidence:** Neither subscription's headroom is known, so the choice rests on
  integration risk: Claude Code's headless mode, JSON output, and credential
  rules were already researched for the landscape document, and its structured
  output is what the isolated worker parses. The escape hatch covers the case
  where headroom turns out to be the binding constraint.
- **Reversed by:** Quota events in the round, or Codex winning the repeated
  cases if the escape hatch fires.

## D-014: Spend caps and the Phase 0 ceiling

- **Date:** 2026-09-14
- **Status:** accepted by delegation
- **Decision:** Metered provider spend (Exa, hosted extractor, any other
  per-call API) is capped at **$15 per day and $150 per month**. The Phase 0
  comparison round has its own one-time ceiling of **$150**. Subscription usage
  by the official CLI is not metered by the service and is observed as quota
  events instead.
- **Alternatives:** Match the incumbent ($200/month); no daily cap; a lower
  monthly cap that would starve `deep` runs.
- **Evidence:** The product has to beat
  $200/month including its own hosting
  to justify replacing Perplexity, so the metered envelope is set below it with
  room for the VM. Exa's fixed efforts run $0.012
  to $1 per task; $15/day allows a dozen `standard` runs or one runaway `max`
  run, which is the failure the daily cap exists to bound. $150 for the round
  covers 16 cases at two efforts plus repeats with margin.
- **Reversed by:** Pilot accounting showing `deep` runs are routinely blocked by
  the daily cap, or the comparison showing the winning effort level costs more
  than assumed.

## D-015: Operating numbers

- **Date:** 2026-09-14
- **Status:** accepted by delegation
- **Decision:** The values first written as "proposed" are adopted unchanged:

  | Setting                                            | Value                                    |
  | -------------------------------------------------- | ---------------------------------------- |
  | Golden set size                                    | 12–16 cases                              |
  | Pilot before cancelling the incumbent              | 2 weeks                                  |
  | Clarification timeout, `standard` / `deep`         | 30 min / 2 h, then proceed on assumption |
  | Freshness windows, volatile / current / stable     | 24 h / 7 d / 90 d                        |
  | Reserved tail for self-review, `standard` / `deep` | 80% / 85% of the duration budget         |
  | Concurrency, hosted tasks / heavy lane             | 2 / 1                                    |
  | Recovery point / recovery time objective           | 1 h / 2 h                                |
  | Backup and source-cache retention                  | 180 days                                 |
  | Debug prompt-log retention                         | 7 days                                   |
  | Disk-pressure threshold                            | 2 GB free                                |

- **Alternatives:** Each could be argued up or down; none has evidence behind it
  yet.
- **Evidence:** These are starting points for a single user on one VM. They are
  configuration, not architecture; the pilot will produce the numbers that
  replace them.
- **Reversed by:** Pilot observations. Any change is a new dated entry here, not
  a silent config edit.

## Pending decisions with dates

Everything that could be decided without evidence has been decided. What remains
waits on Phase 0 results, and deciding it earlier would repeat the original
plan's mistake of choosing before measuring.

| Decision                             | Needed by                             | Blocked on                                  |
| ------------------------------------ | ------------------------------------- | ------------------------------------------- |
| Approve or reject D-003 hosted-first | End of Phase 0                        | Backend comparison results                  |
| Choose D-005 queue                   | Start of Phase 1                      | Phase 0 recovery test                       |
| VM size                              | Start of Phase 1                      | Phase 0 mixed-load measurements             |
| Exa ZDR cell in the D-010 table      | Before first `work` run on Exa search | Phase 0 verification of account ZDR setting |

Setup tasks that follow from accepted decisions (not decisions): create the R2
bucket, its bucket-lock rules, and the scoped token; store and print the restic
password (D-012); confirm Claude Code headless mode works under Robert's
subscription on the VM (D-013).
