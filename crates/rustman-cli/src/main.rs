use clap::Parser;
use rustman_core::{HttpMethod, HttpRequest};

/// rustman - a Postman-parity API client
#[derive(Parser, Debug)]
#[command(name = "rustman", version, about)]
struct Cli {
    /// Name to greet
    #[arg(short, long, default_value = "world")]
    name: String,
}

fn main() {
    let cli = Cli::parse();
    let req = HttpRequest::new("hello", HttpMethod::GET, "https://example.com");
    println!("Hello, {}! rustman is ready (sample request: {:?} {})", cli.name, req.method, req.url);
}
