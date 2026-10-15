#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    runtime_extension_sdk::serve(langfuse::dispatch).await?;
    Ok(())
}
