use ammonia::Builder;
use pulldown_cmark::{Options, Parser, html};
use research_protocol::{Label, ReportEnvelope};

pub fn report_html(envelope: &ReportEnvelope, markdown: &str) -> String {
    let mut rendered = String::new();
    let options = Options::ENABLE_TABLES
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS;
    html::push_html(&mut rendered, Parser::new_ext(markdown, options));
    let safe_body = Builder::default()
        .rm_tags(["img", "picture", "source"])
        .clean(&rendered)
        .to_string();
    let label = label_name(envelope.label.as_ref().unwrap_or(&Label::Draft));
    let claim_numbers = envelope
        .claims
        .iter()
        .enumerate()
        .map(|(index, claim)| (claim.id.as_str(), index + 1))
        .collect::<std::collections::HashMap<_, _>>();
    let claims = envelope
        .claims
        .iter()
        .enumerate()
        .map(|(index, claim)| {
            let outcome = claim
                .review
                .outcome
                .as_ref()
                .map(|value| format!("{value:?}").to_lowercase())
                .unwrap_or_else(|| "unreviewed".into());
            let evidence_caption = match outcome.as_str() {
                "supported" | "qualified" => "Verified passage",
                "unsupported" | "contradicted" | "stale" => {
                    "Cited passage (does not establish this claim)"
                }
                _ => "Candidate passage",
            };
            let evidence = claim
                .evidence
                .iter()
                .filter_map(|id| envelope.evidence.iter().find(|item| item.id == *id))
                .map(|item| {
                    format!(
                        "<blockquote><p>{}</p><footer>{}</footer></blockquote>",
                        escape(&item.quote),
                        evidence_caption
                    )
                })
                .collect::<String>();
            format!(
                "<article class=\"claim\"><header><strong>Claim {}</strong><span class=\"outcome {}\">{}</span></header><p>{}</p>{}</article>",
                index + 1,
                escape(&outcome),
                escape(&outcome),
                escape(&claim.text),
                evidence
            )
        })
        .collect::<String>();
    let limitations = envelope
        .assessments
        .last()
        .map(|assessment| {
            assessment
                .reasons
                .iter()
                .map(|reason| {
                    format!(
                        "<li>{}</li>",
                        escape(&limitation_text(reason, &claim_numbers))
                    )
                })
                .collect::<String>()
        })
        .unwrap_or_default();
    let title = envelope
        .brief
        .get("question")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("Research report");
    format!(
        r#"<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>{title}</title><style>{css}</style></head>
<body><main><header class="report-header"><div><p class="eyebrow">Research report · revision {revision}</p><h1>{title}</h1></div><strong class="label {label_class}">{label}</strong></header>
<section class="report-body">{safe_body}</section>
<section><h2>Claim review</h2>{claims}</section>
<section class="limitations"><h2>Limitations</h2><ul>{limitations}</ul></section>
<footer class="report-footer"><span>Archived as immutable revision {revision}</span></footer>
</main></body></html>"#,
        title = escape(title),
        css = CSS,
        revision = envelope.revision,
        label_class = label.to_lowercase().replace(' ', "-"),
        label = label,
        safe_body = safe_body,
        claims = claims,
        limitations = limitations,
    )
}

fn label_name(label: &Label) -> &'static str {
    match label {
        Label::Draft => "Draft",
        Label::NeedsReview => "Needs Review",
        Label::Reviewed => "Reviewed",
    }
}

fn escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn limitation_text(reason: &str, claim_numbers: &std::collections::HashMap<&str, usize>) -> String {
    if reason == "material_claims_not_reviewed" {
        return "Material claims have not received semantic review.".into();
    }
    if reason.starts_with("required_question_unanswered:") {
        return "A required verification question remains unanswered.".into();
    }
    if reason == "run_hit_limit" {
        return "The research run reached a configured limit.".into();
    }
    for (prefix, message) in [
        ("material_claim_not_supported:", "is not yet supported"),
        (
            "open_material_claim:",
            "requires correction or more evidence",
        ),
    ] {
        if let Some(id) = reason.strip_prefix(prefix) {
            return claim_numbers
                .get(id)
                .map(|number| format!("Claim {number} {message}."))
                .unwrap_or_else(|| format!("A material claim {message}."));
        }
    }
    reason.replace('_', " ")
}

const CSS: &str = r#"
:root{color-scheme:light;--ink:#18201d;--muted:#63706a;--line:#dce3df;--paper:#fbfcfa;--accent:#165f46;--draft:#9b4b16;--review:#8a6110;--good:#17623c}*{box-sizing:border-box}body{margin:0;background:#edf1ee;color:var(--ink);font:16px/1.6 system-ui,-apple-system,BlinkMacSystemFont,"Segoe UI",sans-serif}main{width:min(900px,100%);min-height:100vh;margin:auto;padding:56px 64px;background:var(--paper);box-shadow:0 0 40px #24372b12}.report-header{display:flex;gap:24px;align-items:flex-start;justify-content:space-between;padding-bottom:28px;border-bottom:2px solid var(--ink)}.eyebrow{margin:0 0 6px;color:var(--muted);font-size:.78rem;font-weight:700;letter-spacing:.09em;text-transform:uppercase}h1{max-width:680px;margin:0;font-family:Georgia,serif;font-size:clamp(2rem,5vw,3.25rem);line-height:1.08}h2{margin-top:2.4rem;font-family:Georgia,serif;font-size:1.5rem}.label{white-space:nowrap;padding:7px 11px;border:1px solid currentColor;border-radius:999px;font-size:.78rem;letter-spacing:.06em;text-transform:uppercase}.label.draft{color:var(--draft);background:#fff4e9}.label.needs-review{color:var(--review);background:#fff8db}.label.reviewed{color:var(--good);background:#eaf8ef}.report-body{padding:26px 0 8px}.report-body table{width:100%;border-collapse:collapse}.report-body th,.report-body td{padding:10px 12px;border:1px solid var(--line);text-align:left}.report-body th{background:#f0f4f1}.claim{margin:14px 0;padding:18px;border:1px solid var(--line);border-radius:10px;background:white}.claim header{display:flex;justify-content:space-between;gap:12px}.claim p{margin-bottom:0}.outcome{color:var(--muted);font-size:.76rem;font-weight:700;text-transform:uppercase}.outcome.unsupported,.outcome.contradicted,.outcome.stale{color:#a12b23}blockquote{margin:14px 0 0;padding:10px 16px;border-left:3px solid var(--accent);background:#f4f7f5}blockquote p{margin:0}blockquote footer{margin-top:6px;color:var(--muted);font-size:.78rem;overflow-wrap:anywhere}.limitations{padding:4px 18px 18px;border-radius:10px;background:#fff8e9}.report-footer{display:flex;flex-direction:column;gap:4px;margin-top:48px;padding-top:18px;border-top:1px solid var(--line);color:var(--muted);font-size:.78rem}.report-footer code{overflow-wrap:anywhere}@media(max-width:600px){main{padding:28px 20px}.report-header{display:block}.label{display:inline-block;margin-top:18px}.claim header{align-items:flex-start;flex-direction:column}.report-body table{display:block;overflow-x:auto}}
@media print{body{background:white}main{width:auto;padding:0;box-shadow:none}.claim{break-inside:avoid}.report-footer{position:running(footer)}}
"#;
