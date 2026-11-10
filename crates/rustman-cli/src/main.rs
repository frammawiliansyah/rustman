mod load;
mod runner;

use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{anyhow, bail, Result};
use clap::{Parser, Subcommand, ValueEnum};
use rustman_core::importers::export_postman;
use rustman_core::{
    AssertionType, HeaderPair, HttpExecutor, HttpMethod, HttpRequest, IterationDataset, MockServer, RequestBody, SqliteStorage,
};
use runner::{run_collection, Reporter, RunOptions};

/// rustman - a Postman-parity API client
#[derive(Parser, Debug)]
#[command(name = "rustman", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Run a collection headlessly (Newman parity)
    Run {
        /// Path to collection JSON (Postman v2.1, OpenAPI or Rustman format)
        collection_path: PathBuf,
        /// Environment JSON (Postman or Rustman format)
        #[arg(short, long)]
        environment: Option<PathBuf>,
        /// Delay between requests in ms
        #[arg(long, default_value_t = 0)]
        delay: u64,
        /// Stop on the first failed request
        #[arg(long)]
        bail: bool,
        /// Output reporter
        #[arg(long, value_enum, default_value_t = Reporter::Cli)]
        reporter: Reporter,
        /// Assert every response has this status code
        #[arg(long)]
        expect_status: Option<u16>,
        /// Assert every response arrives within this many ms
        #[arg(long)]
        max_time: Option<u64>,
        /// Iteration data file (.csv or .json)
        #[arg(short = 'd', long = "iteration-data")]
        iteration_data: Option<PathBuf>,
        /// Number of iterations (default: data rows, or 1)
        #[arg(short = 'n', long = "iteration-count")]
        iteration_count: Option<usize>,
    },
    /// Import an OpenAPI / Postman file into SQLite and/or export it
    Import {
        file: PathBuf,
        /// SQLite database to store the collection in
        #[arg(long)]
        db: Option<PathBuf>,
        /// Export format (printed to stdout unless --output is set)
        #[arg(long, value_enum)]
        export: Option<ExportFormat>,
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// Serve a collection as a local mock server
    Mock {
        /// Path to collection JSON (Postman v2.1, OpenAPI or Rustman format)
        collection_path: PathBuf,
        /// Port to listen on
        #[arg(short, long, default_value_t = 8080)]
        port: u16,
    },
    /// Send a one-shot request
    Request {
        /// HTTP method (GET, POST, ...)
        method: String,
        url: String,
        /// Header as "Key: Value" (repeatable)
        #[arg(short = 'H', long = "header")]
        headers: Vec<String>,
        /// Raw request body
        #[arg(short, long)]
        data: Option<String>,
    },
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum ExportFormat {
    Postman,
    Rustman,
}

fn parse_method(s: &str) -> Result<HttpMethod> {
    Ok(match s.to_ascii_uppercase().as_str() {
        "GET" => HttpMethod::GET,
        "POST" => HttpMethod::POST,
        "PUT" => HttpMethod::PUT,
        "DELETE" => HttpMethod::DELETE,
        "PATCH" => HttpMethod::PATCH,
        "HEAD" => HttpMethod::HEAD,
        "OPTIONS" => HttpMethod::OPTIONS,
        _ => bail!("unsupported HTTP method: {s}"),
    })
}

fn load_iteration_data(path: &std::path::Path) -> Result<IterationDataset> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| anyhow!("cannot read {}: {e}", path.display()))?;
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "csv" => Ok(IterationDataset::from_csv_str(&text)?),
        "json" => Ok(IterationDataset::from_json_str(&text)?),
        _ => bail!("unsupported iteration data file (expected .csv or .json): {}", path.display()),
    }
}

async fn real_main() -> Result<i32> {
    match Cli::parse().command {
        Command::Run { collection_path, environment, delay, bail, reporter, expect_status, max_time, iteration_data, iteration_count } => {
            let collection = load::load_collection(&collection_path)?;
            let env = environment.as_deref().map(load::load_environment).transpose()?;
            let mut assertions = Vec::new();
            if let Some(s) = expect_status {
                assertions.push(AssertionType::StatusCode(s));
            }
            if let Some(t) = max_time {
                assertions.push(AssertionType::MaxResponseTime(t));
            }
            let dataset = iteration_data.as_deref().map(load_iteration_data).transpose()?;
            if let Some(d) = &dataset {
                if d.is_empty() && iteration_count.is_none() {
                    bail!("iteration data file contains no rows");
                }
            }
            let opts = RunOptions {
                delay_ms: delay,
                bail,
                reporter,
                assertions,
                iteration_data: dataset,
                iteration_count,
            };
            Ok(run_collection(&collection, env.as_ref(), &opts).await?.exit_code())
        }
        Command::Import { file, db, export, output } => {
            if db.is_none() && export.is_none() {
                bail!("nothing to do: pass --db and/or --export");
            }
            let collection = load::load_collection(&file)?;
            if let Some(db) = db {
                SqliteStorage::new(&db)?.save_collection(&collection)?;
                eprintln!("Saved collection '{}' to {}", collection.name, db.display());
            }
            if let Some(fmt) = export {
                let text = match fmt {
                    ExportFormat::Postman => export_postman(&collection)?,
                    ExportFormat::Rustman => serde_json::to_string_pretty(&collection)?,
                };
                match output {
                    Some(p) => std::fs::write(p, text)?,
                    None => println!("{text}"),
                }
            }
            Ok(0)
        }
        Command::Mock { collection_path, port } => {
            let collection = load::load_collection(&collection_path)?;
            let server = MockServer::from_collection(&collection);
            let (shutdown, bound) = server.start(port).await?;
            println!("Mock server for '{}' listening on http://127.0.0.1:{bound}", collection.name);
            for r in &server.routes {
                println!("  {:<7} {} -> {}", format!("{:?}", r.method), r.path_pattern, r.response_status);
            }
            println!("Press Ctrl+C to stop.");
            tokio::signal::ctrl_c().await?;
            let _ = shutdown.send(());
            Ok(0)
        }
        Command::Request { method, url, headers, data } => {
            let mut req = HttpRequest::new("one-shot", parse_method(&method)?, url);
            for h in headers {
                let (k, v) = h
                    .split_once(':')
                    .ok_or_else(|| anyhow!("invalid header (expected 'Key: Value'): {h}"))?;
                req.headers.push(HeaderPair { key: k.trim().into(), value: v.trim().into(), enabled: true });
            }
            if let Some(body) = data {
                let content_type = req
                    .headers
                    .iter()
                    .find(|h| h.key.eq_ignore_ascii_case("content-type"))
                    .map(|h| h.value.clone())
                    .unwrap_or_else(|| "text/plain".into());
                req.body = RequestBody::Raw { content_type, body };
            }
            let (resp, _) = HttpExecutor::new().execute(&req).await?;
            eprintln!("HTTP {} · {} ms · {} B", resp.status, resp.elapsed_ms, resp.body.len());
            println!("{}", resp.body);
            Ok(if resp.status >= 400 { 1 } else { 0 })
        }
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    match real_main().await {
        Ok(0) => ExitCode::SUCCESS,
        Ok(_) => ExitCode::FAILURE,
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::from(2)
        }
    }
}
