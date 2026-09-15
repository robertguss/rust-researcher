# Incumbent homeschool report audit

Date: 2026-09-15

## Input custody

- Artifact: `Online-Homeschool-Programs-Research-Guide.pdf` (not committed;
  supplied privately by Robert)
- SHA-256: `d3efdfb223679bb4391408fd25e6e4cff7c18bad8c583fef8b631b9665108a55`
- Length: 9 pages
- Original prompt: unavailable
- Reference evidence set: unavailable

This is therefore an incumbent audit, not a valid same-prompt golden-case score.
Scores that require a reference set must not be inferred from this document.

## Observations

The report is clean and readable, uses repeated comparison tables, and presents
three ranked paths. It silently applies a profile of an advanced eight-year-old
entering fourth grade, located in Pennsylvania, with a classical Christian
background. The supplied PDF does not show where those assumptions came from.

The report names many programs and gives prices, accreditation claims, grade
ranges, formats, and recommendations. It provides URLs, but no quoted passages,
retrieval timestamps, source snapshots, or claim-to-evidence locators. The
decision-changing recommendations therefore cannot be mechanically checked
against acquired evidence.

Option B's explicitly priced baseline is approximately
$659/year before Tynker
and Outschool. That lies inside the stated $500–$900
range, but the lower bound cannot include every named component unless one or
more omitted prices are zero. This is a review flag, not a confirmed arithmetic
error, because the underlying prompt and pricing evidence are unavailable.

## Rubric result

The report fails the citation-support hard gate: unqualified, decision-changing
recommendations have no supporting acquired evidence in the report. Material
accuracy, freshness, and source quality cannot be scored honestly without the
original prompt and dated reference evidence. Presentation is 2/2.
Recommendation usefulness is 0/2 under the protocol because the recommendations
are not inspectably supported, regardless of polish.

This establishes a presentation benchmark and a known evidence failure. It does
not establish backend parity or resolve D-003.

## Inputs still required for the backend comparison

Robert must supply the original homeschool prompt and 11–15 additional real
personal/work prompts (or explicitly approve constructed proxy prompts). The
official Claude Code subscription arm also requires subscription authentication;
an Anthropic API key is not an equivalent substitute for D-013.
