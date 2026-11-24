#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    frontal::engine::init_pgid_reaper();
    frontal::cli::run_from_env().await
}
