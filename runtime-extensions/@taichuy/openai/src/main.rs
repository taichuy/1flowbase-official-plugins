#[tokio::main(flavor = "current_thread")]
async fn main() {
    if let Err(error) = openai_provider::serve_multiplex_worker().await {
        eprintln!("OpenAI multiplex worker failed: {error}");
        std::process::exit(1);
    }
}
