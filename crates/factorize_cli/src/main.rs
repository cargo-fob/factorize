use std::path::PathBuf;

use anyhow::Result;
use factorize_core::{Bundler, BundlerOptions};

#[tokio::main]
async fn main() -> Result<()> {
    let input = std::env::args().nth(1).expect("usage: factorize <entry>");
    let output = Bundler::new(BundlerOptions { input: PathBuf::from(input) }).build().await?;
    println!("{}", output.code);
    Ok(())
}
