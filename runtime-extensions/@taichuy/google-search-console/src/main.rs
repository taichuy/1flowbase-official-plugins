use google_search_console::Runtime;
use runtime_extension_sdk::serve_duplex;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let runtime = Runtime::default();
    serve_duplex(move |request, emitter, _| {
        let runtime = runtime.clone();
        async move { runtime.dispatch(request, emitter).await }
    })
    .await?;
    Ok(())
}
