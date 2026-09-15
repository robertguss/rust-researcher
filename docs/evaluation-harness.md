# Evaluation harness and golden set

Prepared for Robert Guss · September 14, 2026 · Design for discussion, not an
implemented system

## Purpose

Nothing in this project should be selected, shipped, or trusted on the basis of
a single successful run. The harness has three jobs:

1. **Backend selection.** Decide between Exa Agent and one official CLI (and
   later any other backend) on Robert's own tasks, not vendor benchmarks.
2. **Release gate.** A backend, instruction version, or adapter change ships
   only if it does not regress the golden set.
3. **Calibration.** Measure how often worker self-review misses errors, so the
   decision to add a second reviewer is made from data.

The harness is a repository directory plus a small runner. It does not need the
full service to exist; the Phase 0 comparison in the
[architecture proposal](rust-research-architecture.md#10-first-version-scope-and-delivery-order)
runs it against raw backend calls.

## 1. Golden set

Start with **12 to 16 cases**. Each case is a directory:

```
eval/cases/<case-id>/
  brief.json          # the research brief as it would be submitted
  reference.md        # dated reference facts, known traps, acceptable uncertainty
  evidence/           # saved copies of the sources the reference relies on
  rubric-notes.md     # case-specific scoring guidance
```

Required category coverage for the first set:

| Category                      | Example                                                                   | Why it is in the set                                                           |
| ----------------------------- | ------------------------------------------------------------------------- | ------------------------------------------------------------------------------ |
| Consumer/education comparison | The homeschool curriculum prompt                                          | The presentation benchmark; tests tables, pricing, ranked options              |
| Software comparison           | Two or three libraries or tools with version-specific differences         | Tests freshness and the ability to read changelogs, not summaries              |
| Current market question       | Pricing or availability of a category of product or service               | Tests live acquisition of pages that change                                    |
| Paper-heavy question          | A question whose honest answer requires reading two or more papers        | Tests scholarly identity, access level honesty, and preprint caveats           |
| Factual lookup                | Something with one verifiable answer                                      | Tests `lookup` depth and cost; the run should be short                         |
| Genuinely uncertain question  | A question where the right answer is "the evidence is mixed" with reasons | Tests whether the backend manufactures confidence                              |
| Trap case                     | A question whose obvious top search results are wrong or outdated         | Tests skimming versus reading                                                  |
| Follow-up                     | A delta question against a prior case's report                            | Tests inherited context and evidence reuse                                     |
| Historical as-of              | What was true on a stated past date (a price, a regulation, a version)    | Tests whether "current" evidence is wrongly used for a past question           |
| Conflicting sources           | Two credible sources that disagree on a material fact                     | Tests the `contradicts` relation and whether the disagreement is shown         |
| Inaccessible source           | The best source is paywalled or blocked                                   | Tests honest `metadata_only` handling and the searches record                  |
| Location-ambiguous            | A question whose answer depends on jurisdiction, with no location given   | Tests clarification versus a recorded assumption; the silent-Pennsylvania case |

`reference.md` is not a model answer. It lists the facts a correct report must
get right (with dates and saved evidence), the mistakes a plausible bad report
makes, and where uncertainty is acceptable. References are dated; a case whose
facts have moved is updated or retired, not silently kept.

Repeat the three most important cases (homeschool, software comparison, trap) at
least twice per evaluation round. One good run says little about reliability.

### Audit the incumbent first

Before scoring any backend, score the Perplexity homeschool report itself with
the same rubric. It is the presentation benchmark, not ground truth. Knowing its
material-error rate sets the bar honestly and prevents "parity with Perplexity"
from meaning "equally wrong".

## 2. Rubric

Each report is scored on the dimensions below. Scores are 0, 1, or 2 per
dimension; a written note accompanies any 0.

| Dimension                 | 2                                                                   | 0                                                                   |
| ------------------------- | ------------------------------------------------------------------- | ------------------------------------------------------------------- |
| Material accuracy         | No material factual or numerical errors against the reference       | One or more material errors                                         |
| Citation support          | Every material claim traces to acquired evidence that supports it   | Claims cite sources that do not contain the claim, or none          |
| Coverage                  | All `required_questions` answered or honestly marked unanswerable   | A decision-relevant question silently missing                       |
| Source quality            | Primary sources; independent origins where corroboration is claimed | Aggregators, syndicated copies counted as independent               |
| Freshness                 | Current-state claims cite sources within their freshness window     | Stale prices, versions, or offers stated as current                 |
| Calibration               | Uncertainty stated where the reference says it exists               | Confident answers on the uncertain case; hedging on the certain one |
| Recommendation usefulness | Ranked, justified, tied to the user's constraints                   | Generic or unsupported by the report's own evidence                 |
| Presentation              | Readable on a phone and in PDF; tables render; label visible        | Broken tables, missing label, unreadable                            |

### Hard gates

These override the score. A report failing any gate fails the case:

- A fabricated quotation, page number, or source.
- An unqualified, decision-changing recommendation with no supporting evidence
  in the report.
- A material arithmetic error.
- The Draft/Reviewed label absent or contradicted by the prose.

These gates are the accepted product standard
([D-009](decisions.md#d-009-label-computed-by-the-service-three-values-append-only-assessments)).
They are not a guarantee that future reports will be flawless; they are what
"unacceptable" means.

## 3. Measurements recorded per run

Alongside the rubric, the runner records:

- Time from submission to **first useful output** (a summary or partial report a
  human could act on) and to **final artifact**.
- Provider-reported cost, estimated cost, and the gap between them.
- For the official CLI: observed quota or rate-limit events, and whether
  Robert's interactive use of the same account was affected during the run.
- Number of sources acquired, by access level.
- Number of material claims, and how many the self-review record claims to have
  checked.
- Self-review misses: material errors found by the human scorer that the
  worker's review record marked as checked. This number decides whether a second
  reviewer becomes default.
- **Rework time.** Minutes Robert spends re-investigating before he would act on
  the report. Estimated by the scorer from the material claims that had to be
  checked by hand. A polished report that needs forty minutes of verification
  has not saved forty minutes; this is the number that most directly measures
  the product.
- Evidence kind mix: how many material claims rest on `raw` acquisitions,
  `provider_extract`, or nothing.

## 4. The backend comparison (replaces the single Exa task)

**Goal.** Choose the first execution route with evidence from Robert's own
tasks.

**Candidates.** Exa Agent at two effort levels that bracket the `standard`
budget, and **Claude Code** as the official CLI
([D-013](decisions.md#d-013-official-cli-candidate-for-the-comparison)) running
the [execution protocol](research-quality-protocol.md#4-execution-protocol) with
minimal retrieval tools.

**Procedure.**

1. The round's spending ceiling is **$150** of metered provider spend
   ([D-014](decisions.md#d-014-spend-caps-and-the-phase-0-ceiling)). The runner
   refuses to exceed it. If Claude Code produces quota or rate-limit events that
   affect interactive use during the round, the three repeated cases are also
   run on Codex and the observation is recorded.
2. Run every golden case once on every candidate; run the three repeated cases
   twice more.
3. Store raw output, timings, cost, and any auth or quota events per run. For
   the hosted candidate, also record per run whether `grounding` was emitted,
   whether it contained passages or only URLs, and, on a stopped run, whether
   partial output was retrievable. These populate the
   [capability contract](run-and-report-contracts.md#2-backend-capability-contract)
   with observed values.
4. Score blind where practical: strip backend identity before scoring.
5. Tabulate rubric scores, gate failures, medians for time and cost, and
   self-review misses.

**Decision rule.**

- A candidate that fails a hard gate on more than one repeated case is out
  regardless of average score.
- Between survivors, prefer the one with the lower material-error and
  citation-support failure rate, then the lower median rework time. Cost and
  time break ties only within the stated budget.
- Quality thresholds are written down **before** the round's outputs are read:
  the maximum acceptable material-error rate per category and the maximum median
  rework time. Reading the outputs first and then choosing a threshold is not a
  gate.
- If the hosted candidate survives and its output gives enough citation detail
  that the service can acquire and verify the sources behind material claims,
  take it first: it is the shorter path to a daily-usable slice.
- If it does not, move the official-CLI worker to Phase 1. Do not integrate
  Parallel or You.com as a reflex; run them through this same procedure if and
  when they are tried.

**Output.** A short dated results document in `eval/results/` with the tables,
the decision, and the evidence that would reverse it.

## 5. Regression runs

Rerun a subset (at minimum the three repeated cases) whenever any of these
changes: instruction version, backend or backend effort mapping, retrieval
adapter, report normalisation code, or the renderer. A regression is a drop of
more than one point on any dimension or any new gate failure. Regressions block
the change until explained.

Live paid calls are budgeted integration checks, not unit tests. Unit tests use
recorded fixtures; the regression run is a deliberate, authorised spend.

## 6. What this harness does not do

- It does not prove general quality. Eight to twelve cases characterise Robert's
  task mix; they do not certify the system for arbitrary questions.
- It does not replace daily judgement. A Reviewed label means the protocol was
  followed, not that the recommendation is right.
- It does not measure the incumbent's future. Perplexity's reports on the golden
  set are a snapshot for comparison, taken once and dated.
