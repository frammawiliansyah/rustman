//! Headless collection runner (Newman-style).

use std::time::{Duration, Instant};

use anyhow::Result;
use colored::Colorize;
use rustman_core::assertions::evaluate_all;
use rustman_core::{
    AssertionResult, AssertionType, Collection, Environment, HttpExecutor, HttpRequest, Item,
    IterationDataset, VariableResolver,
};
use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum Reporter {
    Cli,
    Json,
}

#[derive(Clone, Debug)]
pub struct RunOptions {
    pub delay_ms: u64,
    pub bail: bool,
    pub reporter: Reporter,
    /// Assertions applied to every request's response.
    pub assertions: Vec<AssertionType>,
    /// Data file rows; row `i % len` feeds iteration `i`.
    pub iteration_data: Option<IterationDataset>,
    /// Number of iterations (default: dataset length, or 1 without data).
    pub iteration_count: Option<usize>,
}

impl RunOptions {
    pub fn effective_iterations(&self) -> usize {
        match (self.iteration_count, &self.iteration_data) {
            (Some(n), _) => n,
            (None, Some(d)) => d.len(),
            (None, None) => 1,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct AssertionReport {
    pub name: String,
    pub passed: bool,
    pub message: Option<String>,
}

impl From<AssertionResult> for AssertionReport {
    fn from(r: AssertionResult) -> Self {
        Self { name: r.name, passed: r.passed, message: r.message }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct RequestReport {
    /// 1-based iteration number.
    pub iteration: usize,
    pub folder: Vec<String>,
    pub name: String,
    pub method: String,
    pub url: String,
    pub status: Option<u16>,
    pub elapsed_ms: u64,
    pub size_bytes: usize,
    pub error: Option<String>,
    pub assertions: Vec<AssertionReport>,
}

impl RequestReport {
    /// A request fails on transport error, HTTP status >= 400, or a failed assertion.
    pub fn failed(&self) -> bool {
        self.error.is_some()
            || self.status.map_or(true, |s| s >= 400)
            || self.assertions.iter().any(|a| !a.passed)
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct RunSummary {
    pub collection: String,
    pub iterations: usize,
    pub total: usize,
    pub passed: usize,
    pub failed: usize,
    pub assertions_total: usize,
    pub assertions_failed: usize,
    pub total_duration_ms: u64,
    pub avg_response_ms: u64,
    pub requests: Vec<RequestReport>,
}

impl RunSummary {
    pub fn exit_code(&self) -> i32 {
        if self.failed > 0 { 1 } else { 0 }
    }
}

enum Step<'a> {
    Folder(usize, &'a str),
    Request(Vec<String>, usize, &'a HttpRequest),
}

fn flatten<'a>(items: &'a [Item], depth: usize, path: &mut Vec<String>, out: &mut Vec<Step<'a>>) {
    for item in items {
        match item {
            Item::Request(r) => out.push(Step::Request(path.clone(), depth, r)),
            Item::Folder(f) => {
                out.push(Step::Folder(depth, &f.name));
                path.push(f.name.clone());
                flatten(&f.items, depth + 1, path, out);
                path.pop();
            }
        }
    }
}

pub async fn run_collection(
    collection: &Collection,
    env: Option<&Environment>,
    opts: &RunOptions,
) -> Result<RunSummary> {
    let cli = opts.reporter == Reporter::Cli;
    let executor = HttpExecutor::new();

    let mut steps = Vec::new();
    flatten(&collection.items, 0, &mut Vec::new(), &mut steps);

    if cli {
        println!("{}\n", collection.name.bold());
    }

    let iterations = opts.effective_iterations();
    let started = Instant::now();
    let mut reports: Vec<RequestReport> = Vec::new();
    let mut first = true;
    let mut bailed = false;

    'iters: for iter in 0..iterations {
        let mut resolver = VariableResolver::new();
        if let Some(e) = env {
            resolver = resolver.with_environment_model(e);
        }
        if let Some(row) = opts
            .iteration_data
            .as_ref()
            .and_then(|d| if d.is_empty() { None } else { d.get_row(iter % d.len()) })
        {
            resolver = resolver.with_iteration_data(row.clone());
        }
        if cli {
            println!("{}\n", format!("Iteration {} of {}", iter + 1, iterations).bold().underline());
        }
        for step in &steps {
            match step {
                Step::Folder(depth, name) => {
                    if cli {
                        println!("{}{} {}", "  ".repeat(*depth), "❏".cyan(), name.bold());
                    }
                }
                Step::Request(folder, depth, req) => {
                    if !first && opts.delay_ms > 0 {
                        tokio::time::sleep(Duration::from_millis(opts.delay_ms)).await;
                    }
                    first = false;
                    let mut report =
                        run_one(&executor, &resolver, req, folder.clone(), &opts.assertions).await;
                    report.iteration = iter + 1;
                    if cli {
                        print_report(&report, *depth);
                    }
                    let failed = report.failed();
                    reports.push(report);
                    if failed && opts.bail {
                        bailed = true;
                        break 'iters;
                    }
                }
            }
        }
    }
    if bailed && cli {
        println!("\n{}", "Bailing out: request failed.".red());
    }

    let total = reports.len();
    let failed = reports.iter().filter(|r| r.failed()).count();
    let responded: Vec<u64> = reports.iter().filter(|r| r.status.is_some()).map(|r| r.elapsed_ms).collect();
    let avg = if responded.is_empty() { 0 } else { responded.iter().sum::<u64>() / responded.len() as u64 };
    let summary = RunSummary {
        collection: collection.name.clone(),
        iterations,
        total,
        passed: total - failed,
        failed,
        assertions_total: reports.iter().map(|r| r.assertions.len()).sum(),
        assertions_failed: reports.iter().flat_map(|r| &r.assertions).filter(|a| !a.passed).count(),
        total_duration_ms: started.elapsed().as_millis() as u64,
        avg_response_ms: avg,
        requests: reports,
    };

    match opts.reporter {
        Reporter::Cli => print_summary(&summary),
        Reporter::Json => println!("{}", serde_json::to_string_pretty(&summary)?),
    }
    Ok(summary)
}

async fn run_one(
    executor: &HttpExecutor,
    resolver: &VariableResolver,
    req: &HttpRequest,
    folder: Vec<String>,
    assertions: &[AssertionType],
) -> RequestReport {
    let resolved = resolver.resolve_request(req);
    let mut report = RequestReport {
        iteration: 1,
        folder,
        name: req.name.clone(),
        method: format!("{:?}", resolved.method),
        url: resolved.url.clone(),
        status: None,
        elapsed_ms: 0,
        size_bytes: 0,
        error: None,
        assertions: Vec::new(),
    };
    match executor.execute(&resolved).await {
        Ok((resp, _metrics)) => {
            report.status = Some(resp.status);
            report.elapsed_ms = resp.elapsed_ms;
            report.size_bytes = resp.body.len();
            report.assertions = evaluate_all(assertions, &resp).into_iter().map(Into::into).collect();
        }
        Err(e) => report.error = Some(e.to_string()),
    }
    report
}

fn print_report(r: &RequestReport, depth: usize) {
    let pad = "  ".repeat(depth);
    println!("{pad}{} {} {}", "→".dimmed(), r.method.bold(), r.url);
    match (&r.error, r.status) {
        (Some(e), _) => println!("{pad}  {} {}", "[ERROR]".red().bold(), e.red()),
        (None, Some(s)) => {
            let code = format!("{s}");
            let code = if s < 400 { code.green() } else { code.red() };
            println!("{pad}  {} · {} ms · {} B", code, r.elapsed_ms, r.size_bytes);
        }
        _ => {}
    }
    for a in &r.assertions {
        let tag = if a.passed { "[PASS]".green() } else { "[FAIL]".red() };
        let msg = a.message.as_deref().filter(|_| !a.passed).map(|m| format!(" — {m}")).unwrap_or_default();
        println!("{pad}  {tag} {}{msg}", a.name);
    }
    println!();
}

fn print_summary(s: &RunSummary) {
    let rows = [
        ("iterations", s.iterations.to_string()),
        ("total requests", s.total.to_string()),
        ("passed", s.passed.to_string()),
        ("failed", s.failed.to_string()),
        ("assertions (failed/total)", format!("{}/{}", s.assertions_failed, s.assertions_total)),
        ("total duration", format!("{} ms", s.total_duration_ms)),
        ("avg response time", format!("{} ms", s.avg_response_ms)),
    ];
    let w = rows.iter().map(|(k, _)| k.len()).max().unwrap_or(0);
    let vw = rows.iter().map(|(_, v)| v.len()).max().unwrap_or(0);
    let line = format!("+-{}-+-{}-+", "-".repeat(w), "-".repeat(vw));
    println!("{line}");
    for (k, v) in &rows {
        let cell = format!("| {k:<w$} | {v:>vw$} |");
        if *k == "failed" && s.failed > 0 {
            println!("{}", cell.red());
        } else {
            println!("{cell}");
        }
    }
    println!("{line}");
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustman_core::HttpMethod;
    use rustman_core::{Folder};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    use uuid::Uuid;

    async fn server() -> String {
        let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = l.local_addr().unwrap();
        tokio::spawn(async move {
            loop {
                let (mut s, _) = l.accept().await.unwrap();
                tokio::spawn(async move {
                    let mut buf = [0u8; 2048];
                    let n = s.read(&mut buf).await.unwrap_or(0);
                    let line = String::from_utf8_lossy(&buf[..n]).to_string();
                    let (code, text) = if line.contains("/missing") { (404, "nope") } else { (200, "hello") };
                    let _ = s.write_all(format!("HTTP/1.1 {code} X\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{text}", text.len()).as_bytes()).await;
                });
            }
        });
        format!("http://{addr}")
    }

    fn coll(paths: &[&str]) -> Collection {
        let reqs = paths
            .iter()
            .map(|p| Item::Request(HttpRequest::new(*p, HttpMethod::GET, format!("{{{{base}}}}{p}"))))
            .collect();
        Collection {
            id: Uuid::new_v4(),
            name: "c".into(),
            description: None,
            items: vec![Item::Folder(Folder { id: Uuid::new_v4(), name: "f".into(), description: None, items: reqs })],
        }
    }

    fn env(base: &str) -> Environment {
        Environment {
            id: Uuid::new_v4(),
            name: "e".into(),
            variables: vec![rustman_core::Variable { key: "base".into(), value: base.into(), enabled: true, secret: false }],
        }
    }

    fn opts(bail: bool, assertions: Vec<AssertionType>) -> RunOptions {
        RunOptions { delay_ms: 0, bail, reporter: Reporter::Json, assertions, iteration_data: None, iteration_count: None }
    }

    #[tokio::test]
    async fn all_pass_exit_zero() {
        let base = server().await;
        let s = run_collection(&coll(&["/a", "/b"]), Some(&env(&base)), &opts(false, vec![AssertionType::StatusCode(200)])).await.unwrap();
        assert_eq!((s.total, s.passed, s.failed, s.exit_code()), (2, 2, 0, 0));
        assert_eq!(s.assertions_total, 2);
    }

    #[tokio::test]
    async fn failure_exit_one_and_bail() {
        let base = server().await;
        let c = coll(&["/missing", "/b"]);
        let s = run_collection(&c, Some(&env(&base)), &opts(false, vec![])).await.unwrap();
        assert_eq!((s.total, s.failed, s.exit_code()), (2, 1, 1));
        let s = run_collection(&c, Some(&env(&base)), &opts(true, vec![])).await.unwrap();
        assert_eq!(s.total, 1);
    }

    #[tokio::test]
    async fn multi_iteration_with_data() {
        let base = server().await;
        let c = coll(&["/{{path}}"]);
        let mut o = opts(false, vec![]);
        o.iteration_data = Some(IterationDataset::from_csv_str("path\nok\nmissing\nok2\n").unwrap());
        let s = run_collection(&c, Some(&env(&base)), &o).await.unwrap();
        assert_eq!(s.iterations, 3);
        assert_eq!((s.total, s.passed, s.failed), (3, 2, 1));
        assert_eq!(s.requests.iter().map(|r| r.iteration).collect::<Vec<_>>(), vec![1, 2, 3]);
        assert!(s.requests[1].url.ends_with("/missing"));
    }

    #[tokio::test]
    async fn iteration_count_without_data_and_cycling() {
        let base = server().await;
        let mut o = opts(false, vec![]);
        o.iteration_count = Some(3);
        let s = run_collection(&coll(&["/a", "/b"]), Some(&env(&base)), &o).await.unwrap();
        assert_eq!((s.iterations, s.total), (3, 6));

        let mut o = opts(false, vec![]);
        o.iteration_data = Some(IterationDataset::from_json_str(r#"[{"path":"x"},{"path":"y"}]"#).unwrap());
        o.iteration_count = Some(3);
        let s = run_collection(&coll(&["/{{path}}"]), Some(&env(&base)), &o).await.unwrap();
        let urls: Vec<_> = s.requests.iter().map(|r| r.url.rsplit('/').next().unwrap().to_string()).collect();
        assert_eq!(urls, vec!["x", "y", "x"]);
    }

    #[tokio::test]
    async fn bail_stops_all_iterations() {
        let base = server().await;
        let mut o = opts(true, vec![]);
        o.iteration_count = Some(3);
        let s = run_collection(&coll(&["/missing"]), Some(&env(&base)), &o).await.unwrap();
        assert_eq!(s.total, 1);
    }
}
