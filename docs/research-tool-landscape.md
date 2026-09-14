# A personal research CLI: landscape, tradeoffs, and recommendation

Prepared for Robert Guss · September 14, 2026

**Scope:** comparison and design recommendations for discussion, not
implementation.

## Executive recommendation

Build a small custom research CLI around **your existing Exa account, the
official Claude Code/Codex CLIs, an evidence store, and a supervised background
worker**. Reuse retrieval and document-processing libraries; learn from
open-source research agents rather than forking an entire Perplexity clone.

However, **benchmark a hosted research API before committing to the full
build**. Parallel and You.com sell research by the run at prices that can be far
below $200/month. They do not leverage your subscriptions, but may still be the
cheapest way to achieve your actual goal: high-quality research without another
expensive subscription. Their report quality relative to your Perplexity example
is untested here.

My initial component choices:

| Responsibility                  | Initial choice                                        | Why                                                                          |
| ------------------------------- | ----------------------------------------------------- | ---------------------------------------------------------------------------- |
| General search and URL contents | Exa                                                   | You already use it; no reason to replace it without comparative evidence     |
| Additional search perspective   | Evaluate Brave or Parallel Search                     | Add only if it finds useful sources Exa misses                               |
| Difficult webpage extraction    | Hosted Firecrawl, on demand                           | Less operational work than self-hosting its full stack                       |
| Academic discovery              | arXiv + OpenAlex; evaluate Firecrawl Research Index   | Direct scholarly identifiers and metadata, not just web snippets             |
| Public PDF parsing              | Docling                                               | Local processing and structured output with page provenance                  |
| Research reasoning/writing      | One official Claude Code or Codex CLI session per job | Use existing subscription authentication without pretending it is an API key |
| Background execution            | One supervised worker, SQLite, per-job files          | Sufficient starting point for a personal VM                                  |
| Deliverables                    | Markdown, structured JSON, PDF                        | One evidence-backed report represented for agents and humans                 |
| Notifications                   | Telegram, outbound only                               | Completion/failure notices; no bot command interface                         |

The difficult part is **research quality and recoverability**, not calling a
search API. Finding URLs, actually reading them, preserving evidence,
identifying gaps, and producing defensible recommendations are different steps.

## 1. Requirements established in our discussion

- General-purpose research: software, technical subjects, markets, consumer
  decisions, and academic literature.
- CLI-first, usable by Claude Code and Codex; MCP is optional rather than the
  primary interface.
- Background jobs on a personal exe.dev VM that survive disconnecting the
  initiating session.
- Most reports should target 5–10 minutes, some around 20 minutes; longer
  investigations should be intentional.
- Public web only for now, including publicly accessible PDFs. No login-gated
  content or uploaded-document workflow required initially.
- Telegram notifications only; reports remain available as files.
- Markdown/JSON for agents, PDF for human reading.
- Existing Anthropic/OpenAI subscriptions should provide the reasoning where
  supported. Paid retrieval services are acceptable; Exa is already in use.

Your nine-page homeschool report is a useful presentation benchmark: tailored
comparisons, pricing, caveats, assembled curriculum options, and ranked
recommendations. Its provider links are not a complete claim-level evidence
trail. Its factual accuracy was not audited. It also includes Pennsylvania
context absent from the supplied prompt; that may have come from other
conversation context, but a new system should record such assumptions rather
than silently invent them.

## 2. Can the subscriptions actually power this?

**There is a documented CLI route, but not a general entitlement to use
subscription tokens in arbitrary model clients.** Keep the published agent
binary in charge of inference and authentication.

### Claude Code

Official documentation describes:

- Pro/Max subscription sign-in through Claude Code.
- Noninteractive `claude -p` execution, JSON and JSONL output,
  schema-constrained responses, and resuming named sessions.
- A subscription-authenticated `claude setup-token` flow for scripts/CI,
  documented as producing a one-year OAuth token.

Important constraints:

1. Current `--bare` mode does **not** use subscription OAuth. Do not copy an
   API-oriented automation example blindly. The documentation also says bare
   mode is intended to become the default for print mode in a future release, so
   pin and test CLI behavior.
2. `ANTHROPIC_API_KEY` takes precedence in noninteractive mode when present.
   Accidentally inheriting it could turn a subscription job into a billed API
   job.
3. Ordinary individual usage limits still apply. Long research runs compete with
   your coding use; no promise of unlimited capacity.
4. Anthropic distinguishes an end user signing into the unmodified CLI from
   third parties collecting credentials or offering Claude.ai login in their own
   applications. The latter is not a shortcut to subscription-funded inference.
5. A personally operated CLI worker is the design target. This is not a blanket
   legal clearance for a commercial research service or shared account backend.

Sources: [authentication](https://code.claude.com/docs/en/authentication),
[programmatic CLI](https://code.claude.com/docs/en/headless),
[legal and credential rules](https://code.claude.com/docs/en/legal-and-compliance),
[Agent SDK overview](https://code.claude.com/docs/en/agent-sdk/overview).

### Codex

Official documentation describes `codex exec`, JSONL event output, JSON Schema
output, explicit sandbox settings, session resume, and reuse of saved CLI
authentication. It recommends API keys as the default for automation **but also
documents an advanced ChatGPT-managed authentication path for trusted private
runners when account entitlements/rate limits are needed**.

Treat saved authentication as a password, preserve refreshes securely, and never
expose this runner publicly. A default read-only sandbox also means the eventual
worker needs deliberately configured access to its output directory and
retrieval tools.

Sources: [authentication](https://developers.openai.com/codex/auth),
[noninteractive mode and account-auth automation](https://developers.openai.com/codex/noninteractive).

### Practical consequence

The custom CLI should supply search/read/evidence tools. Claude Code or Codex
should conduct the research through those tools. Do **not** build an
OAuth-to-OpenAI-compatible proxy or assume an SDK's `base_url` setting makes
subscriptions usable.

This was a documentation review, not an authenticated test on your accounts or
VM. Before implementation expands, verify one private background run, its active
billing/authentication mode, tool permissions, restart behavior, and quota
exhaustion handling. Do not silently fall back to paid model APIs.

## 3. Open-source research applications: what is worth learning from?

These are source-inspection findings, not head-to-head quality benchmarks. None
of the inspected projects provides the requested subscription-CLI worker as its
existing model transport.

### GPT Researcher — strongest feature reference

**Repository:**
[assafelovic/gpt-researcher](https://github.com/assafelovic/gpt-researcher) ·
**License:**
[Apache-2.0](https://github.com/assafelovic/gpt-researcher/blob/master/LICENSE).

- Plans questions, gathers sources concurrently, summarizes, and writes reports.
- Has a real Exa retriever and academic adapters including arXiv, Semantic
  Scholar, PMC, and OpenAlex.
- Handles PDF retrieval and offers Markdown/PDF/DOCX output through its CLI.
- Model work is spread across a LangChain-based provider layer; defaults also
  introduce embeddings. Subscription CLIs are not drop-in replacements.

**Learn/reuse:** source normalization, scholarly routing, PDF handling,
report/export patterns, breadth/depth controls. **Avoid initially:** adopting
its entire frontend/server/provider stack. Adaptation burden is medium-high.

Concrete evidence:
[retriever factory](https://github.com/assafelovic/gpt-researcher/blob/master/gpt_researcher/actions/retriever.py),
[Exa adapter](https://github.com/assafelovic/gpt-researcher/blob/master/gpt_researcher/retrievers/exa/exa.py),
[scraper routing](https://github.com/assafelovic/gpt-researcher/blob/master/gpt_researcher/scraper/scraper.py),
[CLI exports](https://github.com/assafelovic/gpt-researcher/blob/master/cli.py),
[default model configuration](https://github.com/assafelovic/gpt-researcher/blob/master/gpt_researcher/config/variables/default.py).

### Open Deep Research — strongest orchestration reference

**Repository:**
[langchain-ai/open_deep_research](https://github.com/langchain-ai/open_deep_research)
· **License:**
[MIT](https://github.com/langchain-ai/open_deep_research/blob/main/LICENSE).

- Clarifies a question, creates a brief, delegates bounded research tasks,
  compresses findings, and synthesizes a report.
- Uses LangGraph with separate research, summarization, compression, and writing
  model roles.
- Current native search choices include Tavily and provider-native search, plus
  MCP extensibility. Exa is not a built-in adapter in the inspected
  implementation.
- No dedicated PDF ingestion/export pipeline was confirmed in the current
  source.

**Learn:** bounded research loops, explicit state, synthesis after source
collection, and evaluation practices. **Avoid initially:** a graph framework or
many separately billed model roles just to imitate its architecture. Adaptation
burden is medium.

Evidence:
[workflow](https://github.com/langchain-ai/open_deep_research/blob/main/src/open_deep_research/deep_researcher.py),
[configuration](https://github.com/langchain-ai/open_deep_research/blob/main/src/open_deep_research/configuration.py),
[search tools](https://github.com/langchain-ai/open_deep_research/blob/main/src/open_deep_research/utils.py).

### dzhng/deep-research — simplest algorithm to understand

**Repository:** [dzhng/deep-research](https://github.com/dzhng/deep-research) ·
**License:** repository
[MIT](https://github.com/dzhng/deep-research/blob/main/LICENSE); package
metadata says ISC, an inconsistency to resolve if redistributing.

- Generates search queries, extracts learnings and follow-up questions,
  recursively explores them, and writes Markdown.
- Small TypeScript implementation; Firecrawl retrieval and API-model providers
  are currently wired into it.
- Appends source URLs, but that is weaker than a claim-to-passage evidence
  model.
- No dedicated academic/PDF pipeline was confirmed.

**Learn:** compact iterative research. **Do not copy blindly:** recursive
breadth/depth can generate unnecessary work, and a URL bibliography does not
establish support for each claim. Lowest algorithm-porting burden of this group,
but still missing much of your desired product.

Evidence:
[research algorithm](https://github.com/dzhng/deep-research/blob/main/src/deep-research.ts),
[model providers](https://github.com/dzhng/deep-research/blob/main/src/ai/providers.ts),
[report writing](https://github.com/dzhng/deep-research/blob/main/src/run.ts).

### Perplexica, now branded Vane — useful product reference, wrong starting shape

**Repository:**
[ItzCrazyKns/Perplexica](https://github.com/ItzCrazyKns/Perplexica) ·
**License:**
[MIT](https://github.com/ItzCrazyKns/Perplexica/blob/master/LICENSE).

- Full search/chat web application with bounded research modes and a streaming
  writer.
- Uses SearxNG in the inspected search implementation. Exa/Tavily are advertised
  as coming rather than verified current adapters.
- Academic search routes to SearxNG engines such as arXiv, Google Scholar, and
  PubMed.
- Supports uploaded PDFs/documents with chunking and embeddings. Report-level
  PDF/Markdown export was not confirmed; a PDF dependency alone is not evidence.

**Learn:** source routing and different research effort modes. **Avoid:**
transplanting a Next.js chat application when your desired interface is a CLI.
Highest CLI adaptation burden here.

Evidence:
[architecture](https://github.com/ItzCrazyKns/Perplexica/blob/master/docs/architecture/README.md),
[research loop](https://github.com/ItzCrazyKns/Perplexica/blob/master/src/lib/agents/search/researcher/index.ts),
[academic search](https://github.com/ItzCrazyKns/Perplexica/blob/master/src/lib/agents/search/researcher/actions/search/academicSearch.ts),
[upload parsing](https://github.com/ItzCrazyKns/Perplexica/blob/master/src/lib/uploads/manager.ts).

### PaperQA2 — valuable academic evidence patterns

**Repository:**
[Future-House/paper-qa](https://github.com/Future-House/paper-qa) · **License:**
[Apache-2.0](https://github.com/Future-House/paper-qa/blob/main/LICENSE).

- Literature-oriented retrieval, chunk contextualization, reranking, and answers
  with paper/page citations.
- Integrates scholarly metadata and local indexing, but model inference and
  embeddings normally run through its own model-provider layer.
- Local-model alternatives exist; that is not the same as using Claude
  Code/Codex subscriptions.

**Learn:** evidence selection, paper metadata deduplication, page citations, and
separating source text from generated descriptions. Adopt wholesale only if its
inference architecture becomes acceptable.
[README and configuration examples](https://github.com/Future-House/paper-qa/blob/main/README.md).

### Overall verdict

Use these projects as **reference implementations and selected dependencies**,
not as proof that another agent framework is required. Your official CLI agents
already supply planning, tool use, and context management. Start with one
research session plus an external evidence store; add more orchestration only
when an evaluation shows a concrete failure it would solve.

## 4. Search APIs: shortlist for your use case

Prices below are public USD list prices checked September 14, 2026. Billing
units differ; taxes, account-specific contracts, extras, and future changes are
excluded. No vendor quality ranking is implied.

| Service                                                                      | Relevant published price                                                                          | Strength for this project                                                        | Recommendation                                                               |
| ---------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | ---------------------------------------------------------------------------- |
| [Exa](https://exa.ai/docs/reference/pricing)                                 | Search $7/1,000 requests including up to 10 results; contents $1/1,000 pages **per content type** | Search and contents in one ecosystem, semantic discovery, structured deep search | Default because you already have it                                          |
| [Brave](https://api-dashboard.search.brave.com/documentation/pricing)        | $5/1,000 search requests; $5 monthly free credits                                                 | Independent index and LLM-context endpoint                                       | Candidate secondary engine for source diversity                              |
| [Parallel](https://docs.parallel.ai/resources/pricing)                       | Search fast/turbo $1/1,000; basic/advanced $5/1,000; includes 10 results; extract $1/1,000 URLs   | Objective-oriented excerpts and separate research API                            | Strong comparison candidate missing from the supplied report                 |
| [Tavily](https://docs.tavily.com/documentation/api-credits)                  | PAYGO $0.008/credit; basic search 1 credit, advanced 2; 1,000 monthly free credits                | Integrated search/extract/crawl and common research-framework support            | Viable substitute; not necessary alongside Exa by default                    |
| [Perplexity Search](https://docs.perplexity.ai/docs/getting-started/pricing) | $5/1,000 successful requests; up to 5 queries per request share one billing unit                  | Raw retrieval without paying for Perplexity synthesis                            | Worth testing; using its API does not require keeping the $200 consumer plan |
| [You.com](https://about.you.com/pricing)                                     | Search $5/1,000 calls, up to 100 results; contents $1/1,000 pages                                 | Broad results and a separate cited research product                              | Candidate, especially for buy-versus-build comparison                        |
| [Serper](https://serper.dev/)                                                | $1/1,000 at entry, but a **$50/50,000-credit pack**, valid 6 months                               | Google-derived web results; also advertises Scholar and Patents                  | Useful if Google coverage matters; not literally a $1 entry purchase         |
| [Kagi](https://kagi.com/api/pricing)                                         | Search $12/1,000; extraction $4/1,000 pages                                                       | Lenses and personalized domain ranking                                           | Optional experiment, not a necessary starting dependency                     |

Avoid buying every provider. Test the same queries and compare **useful unique
sources, freshness, primary-source coverage, extraction completeness, latency,
and total cost**. Three engines returning the same syndicated article are not
three independent confirmations.

## 5. Hosted deep research: an important economic alternative

These services perform reasoning themselves, so their costs are separate from
your subscriptions. They may still cost less overall than maintaining a custom
system.

| Product                                                                         | Verified pricing examples per run                                                              | Why evaluate it?                                                                            |
| ------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------- |
| [Parallel Task](https://docs.parallel.ai/resources/pricing)                     | Pro $0.10; Ultra $0.30; Ultra2x $0.60; Ultra8x $2.40                                           | Asynchronous research and cited structured outputs; inspect its field-level evidence design |
| [You.com Research](https://you.com/docs/research/overview)                      | Lite $0.012; Standard $0.05; Deep $0.10; Exhaustive $0.45; Frontier $1.20                      | Markdown, schema-based JSON, source controls, and background execution                      |
| [Exa Agent](https://exa.ai/docs/reference/pricing)                              | Fixed efforts: minimal $0.012, low $0.025, medium $0.10, high $0.50, xhigh $1; auto is metered | Already in your provider ecosystem; benchmark before assembling everything yourself         |
| [Tavily Research](https://docs.tavily.com/documentation/api-credits)            | Mini 4–110 credits; Pro 15–250 credits. At PAYGO: $0.032–$0.88 and $0.12–$2                    | Explicit variable-cost bounds                                                               |
| [Perplexity Agent API](https://docs.perplexity.ai/docs/getting-started/pricing) | Model tokens plus tools; web search $0.0025/invocation and URL fetch $0.0005                   | Relevant paid baseline, but not guaranteed identical to consumer Perplexity reports         |

For example, 100 Parallel Ultra runs would be
$30 at the listed rate; 100 You.com Exhaustive runs would be $45. **These are
arithmetic examples, not evidence that those runs equal your Perplexity
reports.** Advertised latency ranges and vendor benchmark scores are not
guarantees on your prompts.

A hybrid is plausible: own the CLI, report archive, evidence format, and
Telegram workflow; choose either your subscription agent or a paid research
backend per job. Start with one execution route, not all of them.

## 6. Extraction and PDFs: open source versus hosted

| Tool                                                            | License / operating model                                                                                               | What it contributes                                                                             | Main tradeoff                                                                                                                    |
| --------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------- |
| [Trafilatura](https://github.com/adbar/trafilatura)             | Apache-2.0 in current releases; local Python                                                                            | Lightweight HTML-to-text/Markdown/JSON with metadata; no LLM required                           | Does not render JavaScript or understand PDF layout                                                                              |
| [Crawl4AI](https://github.com/unclecode/crawl4ai)               | Apache-style license with [additional attribution requirement](https://github.com/unclecode/crawl4ai/blob/main/LICENSE) | Local Chromium crawling, Markdown, deterministic extraction; optional LLM features              | Browser operations, memory, blocked pages, and attribution obligations become yours                                              |
| [Firecrawl self-hosted](https://github.com/firecrawl/firecrawl) | AGPL-3.0 core; some components/SDKs MIT                                                                                 | Scrape/crawl service and structured output                                                      | Much heavier than a local reader; not managed-service parity                                                                     |
| [Firecrawl hosted](https://www.firecrawl.dev/pricing)           | Paid service; 1,000 recurring free credits/month; Hobby $19/month monthly billing                                       | Managed extraction and browser infrastructure                                                   | Basic scrape 1 credit/page; additional formats/features can multiply costs                                                       |
| [Jina Reader](https://jina.ai/reader/)                          | Hosted API with source repository available                                                                             | Easy URL/PDF-to-model-friendly text                                                             | Keyless reading is rate-limited; token accounting and optional model processing; exact paid price not established in this review |
| [Docling](https://github.com/docling-project/docling)           | MIT code; model licenses separate                                                                                       | Local PDF layout/OCR/table processing; Markdown and lossless JSON; page/bounding-box provenance | CPU-capable but model-heavy; OCR and complex tables can be slow or wrong                                                         |
| [GROBID](https://github.com/kermitt2/grobid)                    | Apache-2.0 code; local Java service                                                                                     | Scientific structure, bibliography and citation extraction into TEI XML with coordinates        | Another service and XML normalization; add when scholarly parsing warrants it                                                    |

**Why I would not self-host Firecrawl first:** its
[own deployment guide](https://github.com/firecrawl/firecrawl/blob/main/SELF_HOST.md)
describes API/workers, Playwright, Redis, RabbitMQ, PostgreSQL and an optional
queue backend. You own authentication, persistence, backups and operations. The
baseline uses bundled Playwright/basic fetch; managed features and unblocking
are not automatically reproduced.

Recommended staged retrieval:

1. Use search-provider contents if complete enough.
2. Read a live page when freshness or completeness matters; a simple HTTP fetch
   plus Trafilatura is an optional local fast path.
3. Escalate difficult HTML to one hosted extractor, initially Firecrawl. Add
   local browser crawling later only if it saves meaningful money or gives
   needed control.
4. Prefer publisher XML or trustworthy HTML full text for papers when available;
   use Docling for PDFs.
5. Preserve source text and page/section locations. If a figure, equation, or
   table fails extraction, mark that limitation and inspect it separately when
   necessary.

PDF **ingestion** and PDF **report export** are separate jobs. Docling helps
read source PDFs; a deterministic Markdown-to-PDF renderer produces your
reports. JSON and Markdown should be generated from the same evidence-backed
report data so formats cannot silently disagree.

## 7. Academic research needs a separate retrieval path

| Source                                                                         | Useful capabilities                                                                                                           | Limitations / costs                                                                                                                                                                                    |
| ------------------------------------------------------------------------------ | ----------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| [arXiv](https://info.arxiv.org/help/api/tou.html)                              | Direct preprint metadata, identifiers, versions, links to public papers                                                       | Legacy APIs: one request every 3 seconds, one connection, across your machines. A preprint is not proof of peer review                                                                                 |
| [OpenAlex](https://developers.openalex.org/api-reference/works/list-works)     | Broad scholarly metadata, DOI links, citation relationships and open-access locations                                         | [Current pricing](https://help.openalex.org/access/example-costs): free account $1/day; searches $1/1,000, cached PDF downloads $10/1,000. Full text is not available for every record                 |
| [Semantic Scholar](https://www.semanticscholar.org/product/api)                | Paper search, references, citations, related-paper recommendations, metadata                                                  | Public access is shared/throttled; introductory API-key limit is 1 request/second. An open-access URL is not a guarantee that retrieval succeeds                                                       |
| [Crossref](https://www.crossref.org/documentation/retrieve-metadata/rest-api/) | DOI resolution, bibliographic metadata, licenses and post-publication updates                                                 | Metadata service, not a universal full-text library                                                                                                                                                    |
| [Europe PMC](https://europepmc.org/RestfulWebService)                          | Life-science search, abstracts, references, open-access full-text XML                                                         | Full-text XML endpoint covers the open-access subset, not every indexed publication                                                                                                                    |
| [Firecrawl Research Index](https://docs.firecrawl.dev/features/research)       | Paper search, metadata, relevant passages, references/citers; documented coverage includes arXiv and major biomedical sources | [Currently advertised free paper endpoints](https://www.firecrawl.dev/pricing); hosted index, not proof the corpus is bundled with self-hosted Firecrawl. Coverage and passage provenance need testing |

Firecrawl's dedicated paper index is materially different from ordinary web
search restricted to academic domains. Its current docs also announce a change
to the `/search` research category on November 16, 2026; use explicit paper
endpoints rather than relying on that changing response shape.

Start with arXiv and OpenAlex plus Exa's general discovery. Evaluate Firecrawl's
paper tools as a potentially convenient alternative/augmentation, and add
Semantic Scholar or Europe PMC where the topics justify them. Do not integrate
six academic providers just to check boxes.

For credible literature reports, retain DOI/arXiv ID and version, publication
type, access level (abstract/partial/full text), extracted locations, and
uncertainty. Distinguish a result reported by the authors from independently
established consensus. Follow citations in both directions where useful;
citation count is not a quality score. Record withdrawal/retraction information
when available, without treating missing flags as proof that none exists.

## 8. What your custom tool actually needs

### Two modes, one retrieval implementation

**Agent tool mode:** commands for search, reading a URL/PDF, finding papers, and
retrieving stored evidence. Return bounded JSON on stdout; logs go to stderr. A
quick search should not launch another agent or enter the background queue.

**Research job mode:** submit a prompt and depth budget, return a job ID, let a
supervised worker launch one official agent session, and persist outputs. Later
commands inspect status, resume, cancel, and export. These are proposed
interfaces, not existing commands.

For a single VM, SQLite plus a job directory and service supervisor is enough to
start. Avoid Kubernetes, a vector database, and a distributed task system until
a demonstrated requirement demands them. Prefer SSH/private access for
submission and retrieval over building a public API in v1.

### Research workflow

1. Convert the prompt into questions and explicit constraints.
2. Discover broadly, then select relevant primary sources.
3. Read sources; do not mistake search snippets for full documents.
4. Save evidence with stable IDs and locations before compressing it.
5. Pursue contradictions and missing information that could change
   recommendations.
6. Write comparisons, recommendations, limitations, and references.
7. Validate citation references, arithmetic, required outputs, and completion
   status.
8. Export and notify through Telegram.

A source record should contain its URL, final URL, title, retrieval time,
publication metadata where known, provider, content hash, extraction status and
source text location. A finding should link to supporting passages, distinguish
observation from inference, and record unresolved conflicts. Store a schema
version in JSON and keep unknown values explicit rather than inventing data to
fill required fields.

Suggested output bundle: `report.md`, `report.json`, `report.pdf`,
`sources.json`, and an event log. A private source cache retains extracts/PDFs
as permitted by source and API terms. Do not assume publicly readable content is
freely redistributable; arXiv explicitly distinguishes personal retrieval from
serving copies to others.

### Quality budgets, not an arbitrary one-hour loop

Proposed starting presets—not measured performance:

| Mode     | Target                                            | Behavior                                                               |
| -------- | ------------------------------------------------- | ---------------------------------------------------------------------- |
| Lookup   | Seconds to a few minutes                          | Few targeted searches; concise sourced answer                          |
| Standard | 5–10 minutes                                      | Broad discovery, primary-source checks, comparison and recommendations |
| Deep     | Around 20 minutes                                 | More gap-filling, contradictions, academic follow-up and verification  |
| Extended | Explicitly requested, potentially an hour or more | Larger questions with a visible budget and resumable work              |

Time is only one budget. Limit retrieval spend, tool calls and concurrent work;
reserve time for synthesis. Stop early if the key questions are answered. At the
budget boundary, produce a clearly marked partial report or checkpoint, not a
misleading completed report. Do not claim that more sources or more agents
necessarily improves quality.

### Reliable unattended execution

- Durable job states and worker leases so a restart does not start duplicate
  jobs.
- Stage checkpoints and cached successful retrievals; retries should not repeat
  an entire expensive investigation.
- Explicit handling for expired login, model quota exhaustion, retrieval rate
  limits, inaccessible documents and malformed output.
- Separate report success from Telegram delivery: retry a failed notification
  without rerunning research.
- Telegram messages should contain job ID, status, a minimal title, and an
  authenticated retrieval location where available—not source contents or
  secrets. Do not imply a Telegram message is end-to-end encrypted.
- Limit access to one job directory, approved tools and required network
  destinations. Treat retrieved pages as untrusted data, never instructions.
  Protect credentials from document-driven tool calls; enforce public-URL
  fetching rules against private IPs/redirects and cap download sizes.

### One agent or two?

Start with one. An optional second agent can examine unsupported conclusions,
missing alternatives, and citation mismatches on deeper jobs. Give it the
evidence and allow independent source checks where justified. Agreement between
two models reading the same flawed summary is not independent verification.
Measure the extra quota and latency before making dual-agent work the default.

## 9. What could this cost?

Illustrative **retrieval-only** Exa scenarios at current public rates. Assume up
to 10 results per search and one content type per additional contents fetch; do
not separately fetch pages whose contents are already returned adequately.

| Job example     | Search calls | Additional contents pages | Retrieval cost |
| --------------- | -----------: | ------------------------: | -------------: |
| Small lookup    |            3 |                         5 |         $0.026 |
| Standard report |           20 |                        40 |          $0.18 |
| Deeper report   |           60 |                       100 |          $0.52 |

100 standard reports plus 20 deeper reports would be $28.40 in this hypothetical
mix, before free credits. Add any hosted extraction plan, VM/storage expense,
OCR compute, and paid research backend use. This is not a forecast of how many
calls quality reports will require.

Your model subscriptions remain existing fixed costs, **not free unlimited
tokens**. Research may displace coding capacity, trigger pauses, or incur
additional charges if separately enabled. Count maintenance time too. If a
hosted backend meets your quality bar for $30–$50/month, a fully custom
researcher is not automatically the economic winner.

## 10. Assessment of the attached Perplexity toolbox report

Its strongest contribution is separating search, synthesis, extraction,
crawling, and browser automation. Its prices for several key services are
supported by the current docs. But it is a landscape overview, not yet a design
for your subscription-first workflow.

Important corrections and qualifications:

1. **Exa does publish numeric rate limits:**
   [10 QPS search/answer and 100 QPS contents](https://exa.ai/docs/reference/rate-limits).
   The report says there are none.
2. **You.com Research is not generically
   $12/1,000:** that is Lite. Standard is $50, Deep $100, Exhaustive $450,
   Frontier $1,200 per 1,000 runs. The
   [research documentation](https://you.com/docs/research/overview) reveals the
   tier distinction absent from its summary.
3. **Serper's low unit price hides an entry commitment:** $50 minimum paid pack
   and six-month validity at the currently listed entry tier.
4. **Firecrawl's free credits really are recurring under current pricing.** But
   “failed requests are not charged” needs qualification: a returned 403/404
   page costs a credit; a scrape returning no result does not.
5. **Exa contents billing is per content type:** requesting text and highlights
   for one URL counts as two, not one.
6. **The Sonar migration notice is real:**
   [current Perplexity docs](https://docs.perplexity.ai/docs/getting-started/pricing)
   say support through September 27, 2026. Do not start a new integration on the
   legacy Sonar Chat Completions path.
7. **Jina's quoted paid token price was not verified from official material.**
   Its current page confirms keyless Reader limits and a new-key token grant,
   but I would not budget using the third-party estimate in the attachment.
8. **Developer anecdotes and review scores do not establish comparative quality
   or reliability.** Claims such as “fastest,” “deepest integration,” or “only
   tool” need a comparable test, not isolated forum comments or marketing.
9. **Missing pieces matter more than another scraper:** subscription execution
   rules, paper metadata/full-text distinctions, page-grounded evidence,
   checkpointing, artifact consistency, and your own quality benchmark.
10. **Parallel and the dedicated Firecrawl Research Index deserve inclusion.**
    Both materially affect the shortlist for this use case.

I did not exhaustively audit every peripheral vendor claim. Diffbot, Apify,
Bright Data, ScrapingBee, Browserbase and SerpApi remain situational options
from the attachment, but their specialized extraction, browser, platform or SERP
features are not needed to justify your first version. Their detailed prices and
review claims are not independently endorsed by this report.

## 11. Recommended next decision

Choose between:

**A. Subscription-first custom researcher — my recommendation for your stated
preference.** Own the tool/evidence layer and run the official agent CLI. More
control and reuse of subscriptions, but more quality engineering and quota
management.

**B. Custom CLI around a hosted research API.** Own submission, archiving,
exports and notifications; buy the research. Faster path, separately billed
reasoning, less control.

**C. Hybrid after evaluation.** Support one subscription worker and one hosted
backend through the same report/archive format. Do not implement every provider
up front.

Before treating any option as a replacement for Perplexity, run a small
comparative evaluation: your homeschool prompt, a software comparison, a market
question, a paper-heavy question, and a quick factual lookup. Compare against
current sources and your own judgment—not just another model's preference. Audit
material claims and calculations, useful coverage, unsupported recommendations,
citation support, elapsed time, incremental spend, and quota impact. Repeat
important cases because one successful run says little about reliability.

**No tools were installed into your agents, no provider accounts were changed,
no paid API jobs were run, and no application was built in this phase.** The
recommendation is based on official documentation, repository inspection, and
the two supplied examples; empirical quality and authenticated VM execution
remain the next validation step.
