# Source acquisition policy

Prepared for Robert Guss · September 14, 2026 · Design for discussion, not an
implemented system

The
[architecture proposal](rust-research-architecture.md#7-pdf-ingestion-and-worker-isolation)
spends two pages on PDFs and almost none on ordinary web pages. Pricing pages,
product documentation, government guidance, and release notes are where stale or
partial content most often invalidates a report. This document defines how
sources are acquired, identified, cached, and treated under their terms. PDF
parsing keeps the contract already in the architecture proposal; it is scheduled
by the
[delivery plan](rust-research-architecture.md#10-first-version-scope-and-delivery-order),
not required for the first slice.

## 1. Acquisition ladder

Every source goes through the same ladder. Each rung records what was tried and
why the next rung was needed.

```diagram
┌─────────────────────────┐
│ 1 Provider contents     │ Exa (or backend) returns text for the URL
└───────────┬─────────────┘
            │ insufficient or freshness class requires live
            ▼
┌─────────────────────────┐
│ 2 Live HTTP fetch       │ reqwest + readability extraction, SSRF-guarded
└───────────┬─────────────┘
            │ empty/boilerplate/JS-shell
            ▼
┌─────────────────────────┐
│ 3 Hosted extractor      │ one provider (Firecrawl initially), on demand
└───────────┬─────────────┘
            │ still unreadable, or blocked
            ▼
┌─────────────────────────┐
│ 4 Explicit failure      │ access_level stays snippet/metadata_only;
│                         │ report marks dependent claims unsupported
└─────────────────────────┘
```

**Rung 1 is sufficient when** the returned text is above a minimum length for
its type, contains the passage the worker intends to quote, and the source's
freshness class does not require a live fetch.

**Rung 2 is mandatory when** the freshness class is `volatile` (section 4), or
the worker will quote a number, date, price, version, or legal condition from
the page, or rung 1 returned less than the page's visible main content (judged
by a heuristic: text length relative to HTML size, presence of a main content
element, absence of "enable JavaScript" markers).

**Rung 3 is used when** rung 2 returns a JavaScript shell or boilerplate. One
hosted extractor, configured, with its own per-day spend cap. Local Chromium
crawling is not added until the number of rung-3 escalations per month justifies
operating a browser.

**Rung 4 is not a retry loop.** After the ladder is exhausted the source keeps
the best access level actually achieved, the failure reason is recorded on the
acquisition (`blocked_403`, `js_shell`, `paywall`, `robots_disallow`, `timeout`,
`too_large`), and any material claim that depended on it is either re-sourced or
marked unsupported. The report's limitations section lists sources that could
not be read.

**Every acquisition records what kind of content it is.** `content_kind` is one
of:

| `content_kind`     | Produced by                                | May anchor a material claim?                                                   |
| ------------------ | ------------------------------------------ | ------------------------------------------------------------------------------ |
| `raw`              | Rung 2 live fetch; a downloaded PDF        | Yes                                                                            |
| `provider_extract` | Rung 1 provider contents; rung 3 extractor | Yes, but the claim is marked as resting on a third party's reading of the page |
| `model_summary`    | A hosted agent's summary of a page it read | No. It is provenance for what the agent believed, not evidence                 |

An acquisition can have several **extractions** (readability text, Docling
layout JSON, a later re-extraction with a newer version). Evidence locators bind
to one extraction, so re-extracting never changes what an old citation meant.

**Searches are recorded, including empty ones.** Every query the service issues
on a run's behalf is stored with its backend, time, result count, returned URLs,
and, for each URL not acquired, the reason (`duplicate_origin`, `paywalled`,
`off_topic_by_worker`, `over_budget`, `excluded_by_brief`). A report's "no
authoritative source was found" is a claim about the searches, and the searches
are its evidence. An agent inspecting a report can see what was looked for and
what was rejected, not only what was used.

## 2. `full_text` does not mean "correctly extracted"

Access level describes how much of the document was obtained, not whether the
extraction is faithful. An extraction therefore also records
`extraction_confidence` from the extractor (Docling reading-order confidence,
readability score) and an `extraction_warnings` list. Material claims taken from
tables, figures, or multi-column layouts on a source with warnings require the
worker to confirm against the original rendering (the PDF page image or the live
page) and record that it did so. The renderer shows a marker on claims whose
evidence carries unresolved warnings.

## 3. Source identity, deduplication, independence

Content-addressed storage saves bytes; it does not establish identity or
independence.

### URL normalisation

Applied before lookup and before storing `canonical_url`:

- Lowercase scheme and host; strip default ports; strip fragments.
- Resolve redirects and record the chain; the final URL is canonical, the
  originals are aliases.
- Strip tracking parameters from a maintained list (`utm_*`, `fbclid`, `gclid`,
  `ref`, and similar). Do **not** strip other query parameters; many
  documentation and pricing pages are keyed on them.
- Honour `<link rel="canonical">` when present and on the same registrable
  domain; record it as an alias otherwise.
- Never collapse `http` and `https` variants without fetching; they can differ.

### Scholarly identity

A paper is identified by DOI when it has one, otherwise arXiv ID with version. A
DOI and an arXiv ID that resolve to the same work are one `source` with two
identifiers. Different arXiv versions are different acquisitions of the same
source. A preprint and its published version are one source with two versions
and different `publication_type`.

### Origin groups

Two URLs that carry the same text from the same author or organisation are one
**origin group** (`origin_group` on the source). Detection is by exact or
near-duplicate hash of the extracted main text, by identical `rel=canonical`, by
newswire or syndication markers, and by the worker's judgement when it notices
the same press release on three sites. Material-claim corroboration in the
[quality protocol](research-quality-protocol.md#5-material-claims) counts origin
groups, not URLs.

## 4. Caching and freshness

Cache keys:

| Cache             | Key                                                                 |
| ----------------- | ------------------------------------------------------------------- |
| Search results    | provider, normalised query, filters, result count, provider options |
| Provider contents | provider, canonical URL, requested content types                    |
| Live fetch        | canonical URL                                                       |
| Extraction        | content hash, extractor, extractor version, options                 |

A cache hit is reused only if it is within the freshness window of the source's
class. Windows are configurable; these are the accepted defaults
([D-015](decisions.md#d-015-operating-numbers)).

| Class       | Examples                                                         | Window        | Live fetch required for material claims  |
| ----------- | ---------------------------------------------------------------- | ------------- | ---------------------------------------- |
| `volatile`  | Pricing pages, availability, offers, "latest version" pages      | 24 hours      | Yes                                      |
| `current`   | Product documentation, government guidance, news                 | 7 days        | Yes, if the claim is about current state |
| `stable`    | Papers, standards, archived posts, specifications with a version | 90 days       | No                                       |
| `immutable` | Content-addressed documents, arXiv versions, DOI-resolved PDFs   | Never expires | No                                       |

Class is assigned by heuristic (URL patterns, page type, presence of prices or
version strings) and can be overridden by the worker with a recorded reason. A
report shows the retrieval time of every source; a `volatile` source older than
its window when the report is read is flagged on the report page.

An explicit `refresh` on a run or a source bypasses the cache and creates a new
acquisition. Old acquisitions are never overwritten.

## 5. Terms, robots, and retention

Public readability does not grant unrestricted caching or redistribution.

- **robots.txt** is fetched and honoured for live fetches (rung 2). A `Disallow`
  is recorded as `robots_disallow` and the source stays at the access level the
  provider gave it. Robots does not govern rung 1, which is the provider's
  responsibility under its own terms.
- **Rate limits.** Per-host concurrency of one and a minimum inter-request delay
  for live fetches; arXiv's documented pacing is enforced globally across the
  service, not per job.
- **Retention.** Source extracts and PDFs are a private cache for one user. They
  are never served to anyone else, never included in a report artifact beyond
  quoted passages, and are subject to a retention policy (keep while any report
  references them, garbage-collect otherwise after 180 days;
  [D-015](decisions.md#d-015-operating-numbers)). Where a provider's terms
  restrict caching, the adapter declares a maximum retention and the service
  enforces it.
- **Denied access** (401, 403, paywall) is a terminal result for that rung. The
  service never attempts credentialed access, cookie reuse, or header spoofing
  to get past it.
- **Prompt injection.** All fetched content is data. Instructions found in pages
  or PDFs are never executed; the worker's tools are constrained by the service,
  not by the prompt. Pages containing obvious injection attempts are flagged on
  the acquisition for the operator's interest.

Robert's prompts and briefs are sensitive even when the sources are public. They
are never sent to a hosted extractor; only URLs are.

## 6. SSRF and egress

Unchanged from the
[architecture proposal](rust-research-architecture.md#9-access-notifications-and-observability):
scheme allow-list, resolution of every hop, rejection of private, link-local,
and metadata ranges at connection time rather than by string check, response
size and time limits, and an egress policy at the OS or network level as well as
in code. The hosted extractor and the PDF renderer are subject to the same
egress policy.

## 7. What is deferred

- Local browser crawling (Crawl4AI or Chromium): only when rung-3 volume
  justifies it.
- Additional search engines for source diversity: only when the evaluation
  harness shows Exa missing sources that matter on the golden set.
- Rich PDF parsing with Docling: scheduled by the delivery plan after the first
  slice is in daily use; the parser contract in the architecture proposal
  stands.
- Additional scholarly providers beyond what the golden set's paper-heavy cases
  actually need.
