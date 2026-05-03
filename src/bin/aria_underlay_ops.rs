use std::error::Error;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    aria_underlay::ops_cli::run(std::env::args().skip(1)).await
}
