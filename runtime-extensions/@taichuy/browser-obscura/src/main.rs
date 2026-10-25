#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let service = browser_obscura::Service::new();
    runtime_extension_sdk::serve(move |request, emitter| {
        let service = service.clone();
        async move { service.dispatch(request, emitter).await }
    })
    .await?;
    Ok(())
}
