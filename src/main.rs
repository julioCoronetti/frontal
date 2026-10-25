#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    frontal::cli::run_from_env().await
}
