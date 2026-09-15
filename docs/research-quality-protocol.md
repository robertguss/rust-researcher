# Research quality protocol

Prepared for Robert Guss · September 14, 2026 · Design for discussion, not an
implemented system

## Why this document exists

The [architecture proposal](rust-research-architecture.md) says the service owns
jobs, sources, evidence, and reports, while an external agent or hosted API owns
investigation strategy. That split is correct, but it leaves a gap: nobody owns
**what "good research" means**. This document fills it. The service does not run
the reasoning loop, but it does own the quality policy: what a run must contain
before it can be labelled anything other than a draft.

Every rule here is enforceable by the service (structural checks), by the worker
(instructions), or by the [evaluation harness](evaluation-harness.md). Rules
that cannot be checked anywhere are not rules; they are hopes.

## 1. The research brief

Every managed run starts from a brief, not a bare prompt. The client supplies
what it knows; the service fills defaults from the
[standing user context](#2-standing-user-context) and records which fields were
supplied, defaulted, or left unknown.

| Field                | Meaning                                                                                                                                                                                   | Required                    |
| -------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------- |
| `question`           | The prompt as written by the user or agent                                                                                                                                                | Yes                         |
| `decision`           | What the user will do with the answer (choose, buy, learn, verify)                                                                                                                        | Yes, or `unknown`           |
| `audience`           | Who reads the report and at what level                                                                                                                                                    | Defaulted from context      |
| `locale`             | Jurisdiction, region, currency, language                                                                                                                                                  | Defaulted from context      |
| `as_of`              | The date the answer should be current to                                                                                                                                                  | Defaults to submission time |
| `required_questions` | Sub-questions that must be answered for the report to count as usable                                                                                                                     | Derived by worker if absent |
| `constraints`        | Budget ranges, platforms, must-haves, deal-breakers                                                                                                                                       | Optional                    |
| `exclusions`         | Topics, vendors, or sources to leave out                                                                                                                                                  | Optional                    |
| `assumptions`        | Facts the user asserts and does not want re-verified                                                                                                                                      | Optional                    |
| `depth`              | `lookup`, `standard`, `deep`, `extended`                                                                                                                                                  | Yes                         |
| `clarification`      | `ask`, `assume`, or `hold` (see [section 3](#3-clarification-policy))                                                                                                                     | Defaults from context       |
| `scope`              | `personal` or `work`; selects the standing context and retention rules                                                                                                                    | Defaults from context       |
| `classification`     | `public`, `personal_sensitive`, `work_confidential`; governs which backends may receive the brief ([outbound policy](operations-runbook.md#9-privacy-classification-and-outbound-policy)) | Defaults from scope         |
| `evidence_policy`    | Source restrictions, primary-source expectation, review level required                                                                                                                    | Defaults by depth           |
| `output`             | Must-have deliverables: comparison table, ranked options, paper list                                                                                                                      | Optional                    |

The brief is stored on the run and reproduced verbatim in the report's
`assumptions` block. If the worker derives `required_questions`, they are
written back to the run before research starts so the completion check in
[section 7](#7-completion-and-labelling) has something to check against.

## 2. Standing user context

Robert should not retype "Pennsylvania, homeschooling family, senior software
engineer, prefers open source, US dollars" on every run. A small, versioned
profile lives in the service and is readable by any backend.

- One profile per `scope` (`personal`, `work`). This separates household facts
  from client and employer facts without becoming a multi-user product. A `work`
  run never reads the `personal` profile and vice versa.
- Stored as a versioned JSON document; every run records the `context_version`
  it used. Changing the profile does not change old reports.
- Fields are limited to what changes research outcomes: location and
  jurisdiction, household or professional facts, default currency and units,
  standing preferences and dislikes, default `audience`, default `clarification`
  policy, and default depth.
- Every field the worker actually applied appears in the report's `assumptions`
  block as `from_context`, distinct from `from_brief` and `derived`. The
  Perplexity homeschool report injected Pennsylvania context silently; this
  system must not.
- The profile is user-readable and editable through the CLI
  (`research context show|set`). It is not secret, but it is personal; it is
  never sent to Telegram and never included in a report artifact except as
  applied assumptions.

## 3. Clarification policy

An unattended job will sometimes discover that the brief is ambiguous in a way
that changes the answer. The
[job lifecycle](rust-research-architecture.md#6-job-lifecycle-and-recovery) has
`blocked` for reauthentication and spending decisions; it needs the same for
clarification.

| Policy   | Behaviour                                                                                                                                                                                                                                                                   |
| -------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `assume` | Worker picks the most plausible interpretation, records it under `assumptions.derived`, continues. Default for `lookup` and `standard`.                                                                                                                                     |
| `ask`    | Worker posts a bounded question set (at most three, each with a proposed default) and the run enters `blocked` with reason `clarification`. A Telegram message carries the questions. The answer arrives through `POST /v1/runs/{id}/clarify` or `research clarify RUN_ID`. |
| `hold`   | As `ask`, but with no timeout fallback; the run waits until answered or cancelled.                                                                                                                                                                                          |

Under `ask`, if no answer arrives within a configurable window (30 minutes for
`standard`, 2 hours for `deep`; [D-015](decisions.md#d-015-operating-numbers)),
the worker continues under `assume` and the report is labelled as having
unanswered clarification questions. The questions and the defaults taken are
recorded on the run.

Hosted backends generally cannot pause mid-run to ask. For them, `ask` means a
short pre-flight pass by a cheap model or by the client agent before submission,
not a mid-run pause. The backend capability contract in
[run and report contracts](run-and-report-contracts.md) records whether mid-run
clarification is supported.

## 4. Execution protocol

This is the contract a worker follows. For an official-CLI worker it becomes the
versioned instruction file. For a hosted backend it is the **acceptance
contract**: the service checks the output against these expectations and labels
the report accordingly, without assuming a detailed prompt forces the provider
to behave this way.

1. **Frame.** Restate the decision, enumerate `required_questions`, list the
   constraints and exclusions, and state what evidence would settle each
   question. Write this to the run before retrieving anything.
2. **Discover broadly.** Search for each question with more than one phrasing.
   Prefer primary sources: vendor documentation, official pricing pages, papers,
   standards, government publications, first-party changelogs. Treat aggregator
   and listicle pages as leads, not evidence.
3. **Read, do not skim.** A search snippet is not a source. Acquire the page or
   document under the [source acquisition policy](source-acquisition-policy.md)
   before citing it. Record the access level actually obtained.
4. **Capture evidence before compressing it.** For every fact that will appear
   in the report, store the quoted passage, its locator, and the extraction ID
   it was taken from first. Summaries are derived from stored evidence, never
   the other way round.
5. **Pursue conflicts and gaps.** When sources disagree, record both and say
   which is more credible and why. When a required question has no adequate
   source, say so rather than answering from general knowledge.
6. **Analyse with the categories kept apart.** Every statement in the report is
   one of: `observation` (a source says X), `calculation` (derived from stated
   inputs, shown), `inference` (the author's reasoning), or `recommendation`.
   The report format carries this tag per claim; prose may blend them for
   readability but the evidence map does not.
7. **Self-review before publishing.** Check every material claim (section 5),
   every table cell, every number, unit, date, and currency, every qualification
   that was in the source but might have dropped out of the summary, and every
   recommendation for support. Record what was checked.
8. **Declare completion honestly.** List which `required_questions` were
   answered, which were not, and which single new fact would most change the
   recommendation.

The worker records `instruction_version`, the resolved backend configuration,
model identifiers when available, and the tool permissions it ran with. A report
produced under an older instruction version stays valid; it is simply labelled
with the version that produced it.

## 5. Material claims

A **material claim** is any statement whose falsity would change a
recommendation, a ranking, a price, a compatibility statement, a date, a legal
or regulatory condition, or a safety statement. Everything in a comparison
table's cells is material by default.

Rules:

- Every material claim maps to at least one evidence record whose acquisition
  actually obtained content (`partial_text` or `full_text`). A `metadata_only`
  citation supports nothing material.
- Two sources that syndicate the same origin count as one. Independence is
  judged by origin, not by URL count. The
  [source acquisition policy](source-acquisition-policy.md) defines how
  syndication is detected.
- Calculations show their inputs. A total, a per-year cost, or a percentage
  change is a `calculation` claim carrying a `derivation`: the input claim IDs,
  the formula or reasoning, and the as-of date. "Tool A is cheaper" is not
  supported by a link to a pricing homepage; it is supported by the two price
  claims, the usage assumption, and the arithmetic.
- Evidence links have a `relation`: `supports`, `contradicts`, or
  `contextualises`. A claim with a `contradicts` link that the review did not
  resolve is unsupported. Counter-evidence is recorded, not dropped.
- A claim about the current state of something (price, latest version, an offer,
  a legal requirement) is material and must cite a source acquired within the
  freshness window for its class. Provider crawl dates do not satisfy this.
- If a material claim cannot be supported, it is either removed or explicitly
  marked `unsupported` in the report. It is never silently retained.

## 6. Review levels

The architecture correctly separates mechanical checks from semantic review.
This section names the levels so that report labels can be honest.

| Level                      | Who or what performs it                                                     | What it establishes                                                                   |
| -------------------------- | --------------------------------------------------------------------------- | ------------------------------------------------------------------------------------- |
| `structural`               | Service, automatically                                                      | Schema valid, IDs resolve, files and hashes agree                                     |
| `mechanical`               | Service, automatically                                                      | Quotations occur in the recorded extract; locators point at the expected page/element |
| `material_claims_reviewed` | Worker self-review, or a second agent                                       | Each material claim was checked against its evidence for support and qualification    |
| `fully_reviewed`           | Human, or a second agent with fresh source access on `deep`/`extended` runs | Whole-report review including framing, omissions, and recommendation logic            |

`structural` and `mechanical` are required before any report is published.
`material_claims_reviewed` is required before a report is labelled anything
other than **Draft**. `fully_reviewed` is optional and recorded when performed.

"A second agent" and "material-claims review" are different things. The first is
one way to perform the second. Worker self-review is acceptable for
`material_claims_reviewed` as long as the review record lists what was checked;
the evaluation harness measures how often self-review misses errors, and that
number decides whether a second agent becomes the default on deeper runs.

Review produces **assessments**, and assessments are append-only. Each records
the reviewer (worker self-review, second agent, human), the claim IDs examined,
the evidence IDs consulted, the outcome per claim (`supported`, `qualified`,
`unsupported`, `contradicted`, `stale`), the review policy version, and the
reviewer's stated limitations. A later assessment does not overwrite an earlier
one; the report shows the latest and the history is retrievable. A review record
never implies certainty; it says what was checked, by whom, against what.

## 7. Completion and labelling

Every published report carries one of three labels, prominently, in every
format:

- **Draft.** Structurally and mechanically valid; material claims not yet
  reviewed, or coverage incomplete, or clarification questions unanswered, or
  the run ended on a budget boundary. Usable for orientation; not for a
  decision.
- **Needs review.** Review was performed and found a problem that is still open:
  a material claim assessed `unsupported`, `contradicted`, or `stale`, or a
  `volatile` source past its freshness window. The affected claims are marked in
  the report. This is a stronger signal than Draft: someone looked and something
  is wrong.
- **Reviewed.** Material claims reviewed against a named review policy version,
  all `required_questions` either answered or explicitly marked unanswerable
  with reasons, no unsupported or contradicted material claims remaining, no
  open clarification.

There is no universal "verified" badge. A report can finish execution, be
incomplete in coverage, pass quotation matching, and still contain an
unsupported recommendation; the three labels plus the completeness field say
which of those is true.

The label is computed by the service from the run's recorded state and the
latest assessments, never asserted by the worker. The report's `completion`
block lists answered and unanswered questions, the budget consumed, whether the
run hit a limit, and the "what would change this" statement from step 8 of the
protocol.

A report never says "complete" in prose while its label says Draft or Needs
review. The renderer enforces this by placing the label in the header of the
Markdown, HTML, and PDF.

### Corrections

When Robert finds an error in a published report, he records a correction
(`research correct RUN_ID --claim c7 "..."`) rather than editing the report. The
correction is an assessment by a human, marks the affected claims, and produces
a new report revision whose header says it supersedes revision N and why. The
old revision is retained and still readable; its citations keep their original
meaning. Corrections are counted by the evaluation harness as self-review
misses.

## 8. Follow-up runs

Daily research is iterative. A follow-up run references a prior run
(`--follow-up RUN_ID`), which does three things:

1. The new brief inherits the prior brief's `locale`, `audience`, `constraints`,
   and applied context unless overridden, and records the inheritance.
2. The prior run's acquisitions, extractions, and evidence are available to the
   worker without re-fetching; reuse is recorded per source so the new report
   can show which evidence is inherited and how old it is.
3. The new report may be a `delta` report: what changed, what is new, what the
   prior recommendation looks like now. Delta reports still satisfy the
   material-claim rules for anything they assert.

Follow-ups form a chain that the CLI and the report page can show. The prior run
is never modified.

## 9. What is deliberately not specified here

- The internal reasoning strategy of any backend. The service checks outputs,
  not thoughts.
- A universal report template. Comparison tables, paper metadata, and ranked
  recommendations are available blocks, not mandatory ones.
- Multi-agent debate. One worker plus recorded self-review is the starting
  point; a second reviewer is added only where the evaluation harness shows
  self-review missing errors at a rate Robert finds unacceptable.
