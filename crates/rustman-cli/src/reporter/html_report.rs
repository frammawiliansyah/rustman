//! Self-contained HTML dashboard report (htmlextra parity).
//! No external assets: CSS, SVG charts and JS are all inlined.

use std::fmt::Write;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::runner::{RequestReport, RunSummary};

pub struct HtmlReporter;

fn esc(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            '\'' => o.push_str("&#39;"),
            c => o.push(c),
        }
    }
    o
}

/// Current UTC time as `YYYY-MM-DD HH:MM:SS UTC`.
fn timestamp_now() -> String {
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0) as i64;
    format_utc(secs)
}

fn format_utc(secs: i64) -> String {
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    // Howard Hinnant's civil-from-days algorithm.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02} UTC", rem / 3600, rem % 3600 / 60, rem % 60)
}

fn fmt_duration(ms: u64) -> String {
    if ms >= 1000 {
        format!("{:.2} s", ms as f64 / 1000.0)
    } else {
        format!("{ms} ms")
    }
}

/// Donut chart in pure SVG using stroke-dasharray on circles.
fn donut(title: &str, pass: usize, fail: usize) -> String {
    const R: f64 = 15.915_494_309_189_533; // circumference = 100
    let total = pass + fail;
    let mut s = String::new();
    let _ = write!(
        s,
        r#"<div class="chart"><h3>{}</h3><svg class="donut" viewBox="0 0 42 42" role="img" aria-label="{}: {} passed, {} failed">"#,
        esc(title),
        esc(title),
        pass,
        fail
    );
    let _ = write!(s, r##"<circle cx="21" cy="21" r="{R}" fill="none" stroke="#2d3748" stroke-width="6"/>"##);
    if total > 0 {
        let pp = pass as f64 * 100.0 / total as f64;
        let fp = 100.0 - pp;
        if pass > 0 {
            let _ = write!(
                s,
                r##"<circle class="seg" cx="21" cy="21" r="{R}" fill="none" stroke="#22c55e" stroke-width="6" stroke-dasharray="{pp:.3} {:.3}" stroke-dashoffset="25"><title>Passed: {pass}</title></circle>"##,
                100.0 - pp
            );
        }
        if fail > 0 {
            let _ = write!(
                s,
                r##"<circle class="seg" cx="21" cy="21" r="{R}" fill="none" stroke="#ef4444" stroke-width="6" stroke-dasharray="{fp:.3} {:.3}" stroke-dashoffset="{:.3}"><title>Failed: {fail}</title></circle>"##,
                100.0 - fp,
                25.0 - pp
            );
        }
        let _ = write!(
            s,
            r#"<text x="21" y="22.5" text-anchor="middle" class="pct">{:.0}%</text>"#,
            pp
        );
    } else {
        s.push_str(r#"<text x="21" y="22.5" text-anchor="middle" class="pct">n/a</text>"#);
    }
    let _ = write!(
        s,
        r#"</svg><div class="legend"><span class="dot pass"></span>Passed {pass} <span class="dot fail"></span>Failed {fail}</div></div>"#
    );
    s
}

fn kv_table(rows: &[(String, String)]) -> String {
    if rows.is_empty() {
        return "<p class=\"muted\">None</p>".into();
    }
    let mut s = String::from("<table class=\"kv\">");
    for (k, v) in rows {
        let _ = write!(s, "<tr><td>{}</td><td>{}</td></tr>", esc(k), esc(v));
    }
    s.push_str("</table>");
    s
}

fn is_skipped(r: &RequestReport) -> bool {
    r.status.is_none() && r.error.is_none()
}

fn request_row(idx: usize, r: &RequestReport) -> String {
    let (state, label) = if is_skipped(r) {
        ("skipped", "SKIPPED")
    } else if r.failed() {
        ("failed", "FAILED")
    } else {
        ("passed", "PASSED")
    };
    let status = r.status.map(|s| s.to_string()).unwrap_or_else(|| "—".into());
    let status_cls = match r.status {
        Some(s) if s < 400 => "ok",
        Some(_) => "bad",
        None => "bad",
    };
    let folder = if r.folder.is_empty() { "—".to_string() } else { r.folder.join(" / ") };
    let mut s = String::new();
    let _ = write!(
        s,
        r#"<div class="req {state}" data-state="{state}" id="req-{idx}"><div class="req-head" role="button" tabindex="0" aria-expanded="false"><span class="iter">#{}</span><span class="folder">{}</span><span class="method m-{}">{}</span><span class="name">{}</span><span class="url">{}</span><span class="status {status_cls}">{}</span><span class="ms">{} ms</span><span class="badge b-{state}">{label}</span></div><div class="req-body">"#,
        r.iteration,
        esc(&folder),
        esc(&r.method.to_lowercase()),
        esc(&r.method),
        esc(&r.name),
        esc(&r.url),
        esc(&status),
        r.elapsed_ms
    );
    let _ = write!(s, "<h4>Request Headers</h4>{}", kv_table(&r.request_headers));
    let _ = write!(s, "<h4>Query Params</h4>{}", kv_table(&r.query_params));
    s.push_str("<h4>Request Body</h4>");
    match &r.request_body {
        Some(b) if !b.is_empty() => {
            let _ = write!(s, "<pre>{}</pre>", esc(b));
        }
        _ => s.push_str("<p class=\"muted\">No body</p>"),
    }
    s.push_str("<h4>Response</h4>");
    if let Some(e) = &r.error {
        let _ = write!(s, "<p class=\"err\">Error: {}</p>", esc(e));
    } else if r.status.is_some() {
        let _ = write!(
            s,
            "<p>Status <b>{}</b> · {} ms · {} bytes</p>",
            esc(&status),
            r.elapsed_ms,
            r.size_bytes
        );
        if let Some(b) = &r.response_body {
            let _ = write!(s, "<pre>{}</pre>", esc(b));
        }
    } else {
        s.push_str("<p class=\"muted\">Request was not executed</p>");
    }
    s.push_str("<h4>Assertions</h4>");
    if r.assertions.is_empty() {
        s.push_str("<p class=\"muted\">No assertions</p>");
    } else {
        s.push_str("<ul class=\"asserts\">");
        for a in &r.assertions {
            let cls = if a.passed { "pass" } else { "fail" };
            let mark = if a.passed { "✔" } else { "✘" };
            let _ = write!(s, "<li class=\"{cls}\">{mark} {}", esc(&a.name));
            if !a.passed {
                if let Some(m) = &a.message {
                    let _ = write!(s, "<div class=\"fail-msg\">{}</div>", esc(m));
                }
            }
            s.push_str("</li>");
        }
        s.push_str("</ul>");
    }
    s.push_str("</div></div>");
    s
}

const CSS: &str = r#"
*{box-sizing:border-box}body{margin:0;font-family:system-ui,-apple-system,Segoe UI,Roboto,sans-serif;background:#0f172a;color:#e2e8f0}
header{padding:24px 32px;background:#1e293b;border-bottom:1px solid #334155}
header h1{margin:0 0 6px;font-size:26px}header .meta{color:#94a3b8;font-size:14px}
main{padding:24px 32px;max-width:1280px;margin:0 auto}
.cards{display:grid;grid-template-columns:repeat(auto-fit,minmax(220px,1fr));gap:16px;margin-bottom:24px}
.card{background:#1e293b;border:1px solid #334155;border-radius:10px;padding:16px}
.card .label{color:#94a3b8;font-size:13px;text-transform:uppercase;letter-spacing:.05em}
.card .value{font-size:30px;font-weight:700;margin-top:6px}
.card .sub{font-size:13px;margin-top:4px}
.pass{color:#22c55e}.fail{color:#ef4444}.muted{color:#94a3b8}
.charts{display:grid;grid-template-columns:repeat(auto-fit,minmax(260px,1fr));gap:16px;margin-bottom:24px}
.chart{background:#1e293b;border:1px solid #334155;border-radius:10px;padding:16px;text-align:center}
.chart h3{margin:0 0 8px;font-size:16px}
.donut{width:180px;height:180px;transition:transform .2s}.donut:hover{transform:scale(1.04)}
.donut .seg{transition:stroke-width .15s}.donut .seg:hover{stroke-width:7.5}
.donut .pct{fill:#e2e8f0;font-size:7px;font-weight:700}
.legend{font-size:14px;margin-top:8px}
.dot{display:inline-block;width:10px;height:10px;border-radius:50%;margin:0 4px 0 12px}
.dot.pass{background:#22c55e}.dot.fail{background:#ef4444}
.tabs{display:flex;gap:8px;margin-bottom:12px}
.tab{background:#1e293b;color:#e2e8f0;border:1px solid #334155;border-radius:8px;padding:8px 16px;cursor:pointer;font-size:14px}
.tab.active{background:#3b82f6;border-color:#3b82f6}
.req{background:#1e293b;border:1px solid #334155;border-left:4px solid #22c55e;border-radius:8px;margin-bottom:8px}
.req.failed{border-left-color:#ef4444}.req.skipped{border-left-color:#eab308}
.req-head{display:flex;gap:12px;align-items:center;padding:10px 14px;cursor:pointer;font-size:14px;flex-wrap:wrap}
.req-head:hover{background:#273449}
.iter{color:#94a3b8;min-width:32px}.folder{color:#94a3b8;min-width:80px}
.url{flex:1;min-width:160px;word-break:break-all;font-family:ui-monospace,monospace;font-size:13px}
.method{padding:2px 8px;border-radius:4px;font-weight:700;font-size:12px;background:#475569;color:#fff}
.m-get{background:#16a34a}.m-post{background:#d97706}.m-put{background:#2563eb}.m-patch{background:#7c3aed}.m-delete{background:#dc2626}
.status.ok{color:#22c55e}.status.bad{color:#ef4444}.status{font-weight:700}
.badge{padding:2px 8px;border-radius:10px;font-size:11px;font-weight:700}
.b-passed{background:#14532d}.b-failed{background:#7f1d1d}.b-skipped{background:#713f12}
.req-body{display:none;padding:4px 18px 16px;border-top:1px solid #334155}
.req.open .req-body{display:block}
h4{margin:14px 0 6px;color:#94a3b8;font-size:13px;text-transform:uppercase}
pre{background:#0f172a;padding:10px;border-radius:6px;overflow:auto;max-height:320px;font-size:12px;white-space:pre-wrap;word-break:break-all}
table.kv{border-collapse:collapse;font-size:13px}table.kv td{border:1px solid #334155;padding:4px 10px;font-family:ui-monospace,monospace}
.asserts{list-style:none;padding:0;margin:0}.asserts li{padding:3px 0}
.fail-msg{margin:2px 0 0 18px;font-size:12px;color:#fca5a5}
.err{color:#fca5a5}.empty{color:#94a3b8;padding:16px;text-align:center;display:none}
footer{text-align:center;color:#64748b;font-size:12px;padding:24px}
"#;

const JS: &str = r#"
(function(){
var reqs=[].slice.call(document.querySelectorAll('.req'));
var tabs=[].slice.call(document.querySelectorAll('.tab'));
var empty=document.getElementById('empty');
function apply(f){var n=0;reqs.forEach(function(r){var s=r.getAttribute('data-state');
var show=f==='all'||(f==='failed'&&s==='failed')||(f==='skipped'&&s==='skipped');
r.style.display=show?'':'none';if(show)n++;});
empty.style.display=n?'none':'block';
tabs.forEach(function(t){t.classList.toggle('active',t.getAttribute('data-filter')===f);});}
tabs.forEach(function(t){t.addEventListener('click',function(){apply(t.getAttribute('data-filter'));});});
reqs.forEach(function(r){var h=r.querySelector('.req-head');
function tog(){var o=r.classList.toggle('open');h.setAttribute('aria-expanded',o);}
h.addEventListener('click',tog);
h.addEventListener('keydown',function(e){if(e.key==='Enter'||e.key===' '){e.preventDefault();tog();}});});
apply('all');
})();
"#;

impl HtmlReporter {
    /// Render a standalone HTML dashboard for a finished run.
    pub fn generate(summary: &RunSummary, collection_name: &str) -> String {
        let name = esc(collection_name);
        let assertions_passed = summary.assertions_total.saturating_sub(summary.assertions_failed);
        let skipped = summary.requests.iter().filter(|r| is_skipped(r)).count();
        let failed_tab = summary.requests.iter().filter(|r| r.failed() && !is_skipped(r)).count();

        let mut h = String::with_capacity(16 * 1024);
        let _ = write!(
            h,
            r#"<!DOCTYPE html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>{name} - Rustman Report</title><style>{CSS}</style></head><body>
<header><h1>{name}</h1><div class="meta">Executed: {} · Total duration: {}</div></header><main>
"#,
            timestamp_now(),
            fmt_duration(summary.total_duration_ms)
        );
        let _ = write!(
            h,
            r#"<section class="cards">
<div class="card"><div class="label">Total Iterations</div><div class="value">{}</div></div>
<div class="card"><div class="label">Total Requests</div><div class="value">{}</div><div class="sub"><span class="pass">{} passed</span> · <span class="fail">{} failed</span></div></div>
<div class="card"><div class="label">Total Assertions</div><div class="value">{}</div><div class="sub"><span class="pass">{} passed</span> · <span class="fail">{} failed</span></div></div>
<div class="card"><div class="label">Average Response Time</div><div class="value">{} ms</div></div>
</section>
"#,
            summary.iterations,
            summary.total,
            summary.passed,
            summary.failed,
            summary.assertions_total,
            assertions_passed,
            summary.assertions_failed,
            summary.avg_response_ms
        );
        let _ = write!(
            h,
            r#"<section class="charts">{}{}</section>"#,
            donut("Request Status", summary.passed, summary.failed),
            donut("Assertion Results", assertions_passed, summary.assertions_failed)
        );
        let _ = write!(
            h,
            r#"<div class="tabs"><button class="tab active" data-filter="all">All Requests ({})</button><button class="tab" data-filter="failed">Failed Only ({failed_tab})</button><button class="tab" data-filter="skipped">Skipped ({skipped})</button></div><section id="requests">"#,
            summary.requests.len()
        );
        for (i, r) in summary.requests.iter().enumerate() {
            h.push_str(&request_row(i, r));
        }
        let _ = write!(
            h,
            r#"</section><div class="empty" id="empty">No requests match this filter.</div></main><footer>Generated by Rustman</footer><script>{JS}</script></body></html>"#
        );
        h
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner::AssertionReport;

    fn req(name: &str, status: Option<u16>, fail_assert: bool) -> RequestReport {
        RequestReport {
            iteration: 1,
            folder: vec!["Users".into()],
            name: name.into(),
            method: "GET".into(),
            url: "http://x/a?q=<1>".into(),
            status,
            elapsed_ms: 42,
            size_bytes: 5,
            error: None,
            assertions: vec![AssertionReport {
                name: "Status is 200".into(),
                passed: !fail_assert,
                message: fail_assert.then(|| "expected 200 got 500".to_string()),
            }],
            request_headers: vec![("Accept".into(), "application/json".into())],
            query_params: vec![("q".into(), "<1>".into())],
            request_body: Some("{\"a\":1}".into()),
            response_body: Some("hello".into()),
        }
    }

    fn summary() -> RunSummary {
        RunSummary {
            collection: "My <Coll>".into(),
            iterations: 2,
            total: 2,
            passed: 1,
            failed: 1,
            assertions_total: 2,
            assertions_failed: 1,
            total_duration_ms: 1500,
            avg_response_ms: 42,
            requests: vec![req("ok", Some(200), false), req("bad", Some(500), true)],
        }
    }

    #[test]
    fn generates_dashboard() {
        let html = HtmlReporter::generate(&summary(), "My <Coll>");
        assert!(html.starts_with("<!DOCTYPE html>"));
        assert!(html.trim_end().ends_with("</html>"));
        assert!(html.contains("My &lt;Coll&gt;"));
        assert!(!html.contains("My <Coll>"));
        for m in ["Total Iterations", "Total Requests", "Total Assertions", "Average Response Time", "42 ms", "1.50 s"] {
            assert!(html.contains(m), "missing {m}");
        }
        assert_eq!(html.matches("<svg class=\"donut\"").count(), 2);
        assert!(html.contains("#22c55e") && html.contains("#ef4444"));
        assert!(html.contains("class=\"req failed\"") && html.contains("class=\"req passed\""));
        assert!(html.contains("expected 200 got 500"));
        assert!(html.contains("Request Headers") && html.contains("Query Params") && html.contains("Request Body"));
        assert!(html.contains("All Requests (2)") && html.contains("Failed Only (1)") && html.contains("Skipped (0)"));
        assert!(!html.contains("http://") || !html.contains("src=\"http"));
        assert!(!html.contains("<link"));
    }

    #[test]
    fn empty_run_and_skipped() {
        let mut s = summary();
        s.requests = vec![req("never", None, false)];
        s.total = 1;
        let html = HtmlReporter::generate(&s, "c");
        assert!(html.contains("Skipped (1)"));
        let mut e = summary();
        e.requests.clear();
        e.passed = 0;
        e.failed = 0;
        e.assertions_total = 0;
        e.assertions_failed = 0;
        assert!(HtmlReporter::generate(&e, "c").contains("n/a"));
    }

    #[test]
    fn utc_format() {
        assert_eq!(format_utc(0), "1970-01-01 00:00:00 UTC");
        assert_eq!(format_utc(1_709_210_096), "2024-02-29 12:34:56 UTC");
    }
}
