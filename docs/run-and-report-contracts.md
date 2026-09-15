# Run, report, and backend contracts

Prepared for Robert Guss · September 14, 2026 · Design for discussion, not an
implemented system

This document resolves the parts of the
[architecture proposal](rust-research-architecture.md) that were promised but
not modelled: partial results, reconciliation of `unknown`, what "the service
enforces limits" means when a backend cannot, the shape of `report.json`, and
the API operations the three workflows implied but did not list.

## 1. Run state is several fields, not one status

A run can finish execution successfully while its report is partial, its PDF
export failed, its material claims are unreviewed, and its Telegram notification
is still retrying. One `status` column cannot say that. The run record carries
these independent dimensions:

| Dimension        | Values                                                                                                                                 | Owner                          |
| ---------------- | -------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------ |
| `execution`      | `queued`, `running`, `succeeded`, `failed`, `blocked`, `unknown`, `cancelled`                                                          | Worker via job lifecycle       |
| `blocked_reason` | `reauth`, `spend_decision`, `quota`, `clarification`, `reconcile`; null unless `blocked`                                               | Worker                         |
| `completeness`   | `none`, `partial`, `complete`                                                                                                          | Service, from completion block |
| `review`         | `structural`, `mechanical`, `material_claims_reviewed`, `fully_reviewed`                                                               | Service, from review records   |
| `label`          | `draft`, `needs_review`, `reviewed`; computed, never set                                                                               | Service                        |
| `artifacts`      | Per format: `pending`, `available`, `failed`, with hash and path                                                                       | Export job                     |
| `notification`   | `pending`, `delivered`, `failed`, `unknown`                                                                                            | Notification job               |
| `spend`          | `estimated`, `reported`, `reconciled` amounts, and `cap`                                                                               | Service and backend            |
| `external`       | `none`, `submitted`, `accepted`, `running`, `terminal`, `unreconciled`; the provider-side view, kept separate from our execution state | Worker                         |

`GET /v1/runs/{id}` returns all of them. The CLI's `research status` prints a
one-line summary derived from them
(`succeeded · partial · draft · md,pdf · notified`) and the full structure with
`--json`.

The lifecycle diagram in the architecture proposal describes `execution` only.
That is correct; it just is not the whole run.

### `blocked` needs an action, not a retry

Each `blocked_reason` has exactly one resolving operation:

| Reason           | Resolving operation                               | What it does                                                                                                            |
| ---------------- | ------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------- |
| `reauth`         | Operator re-authenticates the CLI, then `resume`  | Creates a new attempt reusing checkpoints                                                                               |
| `spend_decision` | `POST /v1/runs/{id}/approve-spend` with a new cap | Raises the cap and resumes; the approval is recorded with who approved and when                                         |
| `quota`          | `resume` after the quota window, or `cancel`      | Never switches accounts or billing modes                                                                                |
| `clarification`  | `POST /v1/runs/{id}/clarify` with answers         | Writes answers into the brief, resumes; see the [quality protocol](research-quality-protocol.md#3-clarification-policy) |
| `reconcile`      | `POST /v1/runs/{id}/reconcile`                    | See below                                                                                                               |

### `unknown` is resolved by reconciliation, not by `resume`

`unknown` means a paid external action may have happened. `resume` alone would
risk a duplicate charge. Reconciliation is explicit:

```
POST /v1/runs/{id}/reconcile
{ "action": "adopt", "external_task_id": "..." }   # the provider did accept it; poll that task
{ "action": "mark_failed" }                        # the provider confirms nothing ran
{ "action": "resubmit", "accept_charge": true }    # authorise a possibly-duplicate paid attempt
```

The worker first attempts automatic reconciliation using whatever the provider
offers (idempotency keys, task listing by client reference). Only when that is
inconclusive does the run enter `blocked` with reason `reconcile`, and the
Telegram message says so in plain words: "Run 42 may have been submitted to Exa;
check the dashboard before resubmitting."

## 2. Backend capability contract

The architecture says the service enforces limits and, later, that some backends
cannot support hard deadlines, cost ceilings, or cancellation. Both are true;
the contract makes the gap visible.

Each backend adapter declares, statically:

| Capability               | Values                                                                                               |
| ------------------------ | ---------------------------------------------------------------------------------------------------- |
| `deadline`               | `hard`, `best_effort`, `unsupported`                                                                 |
| `cost_cap`               | `hard`, `estimated`, `unsupported`                                                                   |
| `cancellation`           | `stops_billing`, `stops_work`, `unsupported`                                                         |
| `cost_reporting`         | `per_run`, `aggregate`, `none`                                                                       |
| `citations`              | `with_passages`, `urls_only`, `none`                                                                 |
| `mid_run_clarification`  | `supported`, `unsupported`                                                                           |
| `resume`                 | `session`, `checkpoint`, `none`                                                                      |
| `structured_output`      | `schema`, `markdown`, `text`                                                                         |
| `partial_output`         | `on_stop`, `on_deadline`, `none`; whether a run stopped early yields usable output                   |
| `result_retention`       | Duration results stay retrievable after completion, or `immediate_delete`                            |
| `submission_idempotency` | `key`, `client_reference`, `none`; what the provider offers for reconciliation                       |
| `privacy_mode`           | Provider-side retention setting in force for this account (for example Exa ZDR) and its side effects |

The contract is bound to the provider API or CLI version, the effort mode, and
the account settings it was verified against; those are recorded on it and on
every run. A capability is not "supported" because documentation says so. It is
supported when the Phase 0 test against Robert's account showed it.

Exa Agent illustrates why every field matters, from its
[current guide](https://exa.ai/docs/reference/agent-api-guide) (checked
September 14, 2026):

- `output.grounding` is emitted "when emitted". Citations are not promised for
  every claim, and passages are not promised at all. The adapter declares
  `citations: urls_only` until a real run shows otherwise, and the service does
  its own acquisition and mechanical checks.
- `budget.maxCostDollars` exists only for metered `auto` and `max` effort; fixed
  efforts have fixed prices and reject a budget. `cost_cap` is therefore `hard`
  for fixed efforts by construction and `hard` (ceiling) for metered ones, and
  the adapter must not accept `--max-cost` below the fixed price.
- Under Zero Data Retention, results are deleted immediately after completion if
  not collected, and `previousRunId` is unavailable. If ZDR is enabled on the
  account, `result_retention` is `immediate_delete`, `resume` is `none`, and a
  worker outage during collection loses the run even though the task ID was
  persisted. The adapter must collect on the completion event, not on a later
  poll, and the runbook must treat this as a known loss mode.
- Whether stopping a running task retains partial output, and for which effort
  modes, was reported by an independent review and is unverified here
  (**verify** in Phase 0 before declaring `partial_output`).

Request handling rule: a request that asks for a hard limit the backend declares
as `best_effort`, `estimated`, or `unsupported` is **rejected** with a stable
error code, unless the request carries `accept_weaker_limits: true`, in which
case the run records that the limit is advisory. The CLI exposes this as
`--accept-weaker-limits`. A hard limit is never silently downgraded.

A request whose `classification` is not permitted for the backend under the
[outbound policy](operations-runbook.md#9-privacy-classification-and-outbound-policy)
is rejected before anything is transmitted.

`research backends` lists the adapters and their declared capabilities so an
agent can choose before submitting.

## 3. Budget semantics

`--max-cost` and `--max-duration` are flags in the architecture proposal. These
are their meanings.

- **Per-run cap.** Reserved at submission from the daily and monthly caps.
  Reconciled against reported cost on completion; the difference is released or
  recorded as overrun.
- **Reservation is atomic and precedes transmission.** The reservation against
  the daily and monthly caps is one SQLite transaction that also records the
  intended provider submission (idempotency key or client reference). Only after
  it commits does the adapter call the provider. Two agents submitting in the
  same second cannot both fit under the last dollar of the cap, and a crash
  between reservation and provider call leaves a record that reconciliation can
  resolve rather than a silent charge.
- **Service-wide concurrency cap.** At most N hosted tasks in flight and M heavy
  lane jobs across all clients (N = 2, M = 1;
  [D-015](decisions.md#d-015-operating-numbers)). Submissions beyond the cap
  queue rather than fan out; an agent loop cannot start twenty runs at once.
- **Daily and monthly caps.** Configured in the service; initial values
  **$15
  per day and $150 per month** of metered provider spend
  ([D-014](decisions.md#d-014-spend-caps-and-the-phase-0-ceiling)), separate
  from the subscriptions that the official CLI runs under. A submission that
  would exceed either is rejected, or enters `blocked` with reason
  `spend_decision` if the request says `--queue-on-cap`.
- **Standalone tool operations count.** `POST /v1/search` and `POST /v1/sources`
  spend against the same daily cap. A quick lookup is cheap, but a hundred of
  them from an agent loop are not.
- **Queue time does not count** toward `--max-duration`. Wall time from first
  worker claim does. The run records both so the user can see queue delay
  separately.
- **Reserved tail.** The worker stops investigation at 80% of the duration
  budget for `standard` and 85% for `deep` to leave time for self-review and
  publication. Reports that hit the boundary are `partial` and say so.
- **Unknown is not zero.** A backend with `cost_reporting: none` produces
  `spend.reported = null`, and the monthly view shows unknown spend as a
  separate line, not as savings.

## 4. Worker lanes

One worker process, but two lanes:

- **Control lane.** Polling hosted tasks, processing cancellations, sending
  notifications, heartbeating leases. Never blocked by heavy work.
- **Heavy lane.** Research execution, parsing, rendering. Bounded concurrency
  (initially one).

A long OCR job must not delay a cancellation or a Telegram retry. In
implementation terms this is two bounded task sets in the same process, not two
processes; the architecture's `serve`/`worker` split is orthogonal.

## 5. Report envelope, version 1

The canonical `report.json` is an envelope around one authoritative Markdown
body, not a bespoke document language. Structured blocks are added when an agent
actually needs cell-level access to them.

```json
{
  "schema_version": "1",
  "run_id": "run_...",
  "revision": 1,
  "supersedes": null,
  "label": "draft",
  "produced_by": {
    "backend": "exa-agent",
    "backend_config": { "effort": "medium" },
    "instruction_version": "2026-09-14.1",
    "context_version": 3,
    "model": null
  },
  "brief": { "...": "the research brief as submitted, plus derived fields" },
  "assumptions": {
    "from_brief": [],
    "from_context": [{ "field": "locale", "value": "US-PA" }],
    "derived": [{ "question": "...", "assumed": "...", "answered": false }]
  },
  "body_markdown_path": "report.md",
  "body_hash": "sha256:...",
  "claims": [
    {
      "id": "c1",
      "kind": "observation",
      "text": "...",
      "material": true,
      "evidence": ["ev_..."],
      "derivation": null,
      "review": { "checked": true, "by": "worker_self_review", "note": null }
    },
    {
      "id": "c2",
      "kind": "calculation",
      "text": "Total annual cost is about $3,400.",
      "material": true,
      "evidence": ["ev_...", "ev_..."],
      "derivation": {
        "inputs": ["ev_...", "ev_..."],
        "method": "sum of per-course tuition from the two cited price pages",
        "as_of": "2026-09-14"
      },
      "review": { "checked": true, "by": "worker_self_review", "note": null }
    }
  ],
  "evidence": [
    {
      "id": "ev_...",
      "extraction": "ex_...",
      "relation": "supports",
      "locator": { "kind": "page", "page": 4 },
      "quote": "...",
      "normalisation": "whitespace,unicode-nfkc",
      "mechanical_check": "passed"
    }
  ],
  "assessments": [
    {
      "at": "...",
      "by": "service",
      "policy_version": "2026-09-14.1",
      "label": "draft",
      "reasons": ["unanswered_required_question:q2"]
    }
  ],
  "sources": [
    {
      "acquisition": "acq_...",
      "extraction": "ex_...",
      "content_kind": "raw",
      "access_level": "full_text",
      "retrieved_at": "...",
      "origin_group": "og_..."
    }
  ],
  "searches": [
    {
      "id": "s_...",
      "backend": "exa-search",
      "query": "...",
      "at": "...",
      "result_count": 0,
      "excluded": [{ "url": "...", "reason": "paywalled" }]
    }
  ],
  "completion": {
    "required_questions": [{ "q": "...", "answered": true }],
    "hit_limit": null,
    "what_would_change_this": "..."
  },
  "review": {
    "level": "material_claims_reviewed",
    "records": [
      {
        "by": "worker_self_review",
        "checked": ["tables", "arithmetic", "dates"],
        "at": "..."
      }
    ]
  },
  "provider_native": { "path": "provider/raw.json", "hash": "sha256:..." },
  "artifacts": {
    "md": { "path": "report.md", "hash": "..." },
    "html": { "path": "report.html", "hash": "..." },
    "pdf": { "path": "report.pdf", "hash": "..." }
  }
}
```

`POST /v1/reports/import` transports this envelope in a JSON wrapper with
`envelope`, `body_markdown` (the bytes named by `body_markdown_path`), and
optional `provider_native` JSON. The wrapper is not part of envelope version 1.
The service verifies the attached hashes and writes the bytes into its private
content-addressed store; an external CLI cannot safely write server-local
artifact paths.

Rules:

- Claims are anchored into the Markdown body by stable inline markers (`[^c1]`
  style footnote references) so the evidence map and the prose cannot drift
  apart. The renderer turns them into footnotes or hover targets.
- `label` is computed by the service from `completion`, `review`, and
  `assumptions.derived`. Any worker-supplied value is ignored. Every computation
  appends an entry to `assessments` with the policy version that produced it;
  entries are never rewritten, so a report's label history survives policy
  changes.
- Evidence `relation` is one of `supports`, `contradicts`, `contextualises`. A
  claim whose evidence includes a `contradicts` entry cannot be labelled
  `reviewed` without a review record that names the disagreement.
- Claims of kind `calculation`, `comparison`, or `projection` carry a
  `derivation` (inputs, method, as-of date). A calculation without one fails the
  mechanical check.
- `searches` records every search issued for the run, including those that
  returned nothing and sources that were found but excluded, with the reason.
  "No source found" is a claim about the search, and it needs its evidence too.
- HTML and PDF are derived from `report.md` and the envelope; they are never
  edited independently. A new revision is a new envelope.
- A correction produces a new revision whose `supersedes` names the previous
  revision. The previous revision stays readable and its manifest is never
  modified; the run's current-revision pointer moves in the same transaction
  that publishes the new one.
- `provider_native` is preserved byte-for-byte alongside. Normalisation must not
  touch it.
- Optional blocks (`tables`, `papers`, `recommendations`) may be added later
  with their own `schema_version` fields. Their absence is normal.

## 6. API additions

Operations implied by the architecture's workflows but missing from its table:

| Operation                    | HTTP                                           | Notes                                                                                                                                  |
| ---------------------------- | ---------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------- |
| Answer clarification         | `POST /v1/runs/{id}/clarify`                   | Body: answers keyed by question ID                                                                                                     |
| Approve additional spend     | `POST /v1/runs/{id}/approve-spend`             | Body: new cap; recorded                                                                                                                |
| Reconcile an unknown run     | `POST /v1/runs/{id}/reconcile`                 | See section 1                                                                                                                          |
| Create a follow-up run       | `POST /v1/runs` with `follow_up_of`            | Inherits brief and evidence per the [quality protocol](research-quality-protocol.md#8-follow-up-runs)                                  |
| Import an external report    | `POST /v1/reports/import`                      | Replaces the v1 client-led lifecycle: submit a finished envelope plus evidence; service validates and labels                           |
| Source job status            | `GET /v1/jobs/{id}`                            | For the `202` returned by `POST /v1/sources`; same event and cancel semantics as runs                                                  |
| List backends                | `GET /v1/backends`                             | Capability contracts                                                                                                                   |
| Read or set standing context | `GET`/`PUT /v1/context`                        | Versioned; one profile per `scope` (`personal`, `work`)                                                                                |
| Operator summary             | `GET /v1/summary`                              | Recent runs, stages, blocked reasons, spend today/month; backs the CLI dashboard and report page                                       |
| Find runs                    | `GET /v1/runs?q=&scope=&label=&after=&cursor=` | Full-text over brief and report body plus filters; paged. The archive is useless if only `GET /v1/runs/{id}` exists                    |
| Inspect evidence             | `GET /v1/runs/{id}/evidence?cursor=&claim=`    | Paged evidence for a run or one claim; each item carries its locator and a link to the extraction text                                 |
| Correct a report             | `POST /v1/runs/{id}/correct`                   | Body: claim IDs, corrected text or evidence, reason. Produces a new revision with `supersedes` set                                     |
| Export a bundle              | `GET /v1/runs/{id}/bundle`                     | Tar of manifest, envelope, body, artifacts, acquisitions, extractions, provider-native output; for archival or moving between machines |
| Re-render a report           | `POST /v1/runs/{id}/rerender`                  | New HTML/PDF from the existing envelope and body; no network, no spend                                                                 |
| Re-check a report            | `POST /v1/runs/{id}/recheck`                   | Re-run mechanical checks and label computation against the current policy version; appends an assessment                               |
| Refresh a report             | `POST /v1/runs/{id}/refresh`                   | Re-acquire the report's sources and mark claims whose evidence has changed; spends on acquisition only                                 |

`resume` continues an interrupted run. The three operations above it do not
resume anything; each has one clearly bounded effect and its own spend profile,
so an agent never has to guess which one it will get.

Tool responses (search results, source text, evidence pages) are truncated at a
declared size. A truncated response carries a `truncated: true` flag, a locator
for where the cut happened, and a `continuation` cursor. An agent that wants the
rest asks for it; it never receives a silently shortened document.

The client-led workflow (create run, attach evidence incrementally, submit
report) is **deferred**. In v1, an agent that does its own research produces a
finished envelope and imports it. The incremental evidence-attach contract is
designed when there is a demonstrated need to hand unfinished investigations
between harnesses.

## 7. Schema evolution

Five things are versioned independently, because they change for different
reasons:

| Artifact          | Versioning                                      | Compatibility rule                                                        |
| ----------------- | ----------------------------------------------- | ------------------------------------------------------------------------- |
| SQL schema        | Numbered SQLx migrations, forward-only          | Migrations run on `serve` start; worker refuses to start on a mismatch    |
| Report envelope   | `schema_version` string                         | Old envelopes render forever; readers support all versions ever shipped   |
| Extraction format | `extraction_version` on `extractions`           | Locators are valid only against their own extraction version              |
| Instruction file  | `instruction_version` on runs                   | Never rewritten in place; new file, new version                           |
| Backend adapter   | Adapter version and pinned provider API version | Capability contract may change; old runs keep the contract they ran under |

In-flight work across an incompatible upgrade: the worker drains (finishes or
checkpoints heavy-lane tasks, then exits) before the binary is replaced. Runs
that were `running` at drain resume as a new attempt on the new version and
record that fact. Hosted tasks continue to be polled by the new worker via their
stored external task IDs.
