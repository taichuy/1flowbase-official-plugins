#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    runtime_extension_sdk::serve(github_login::dispatch).await?;
    Ok(())
}
