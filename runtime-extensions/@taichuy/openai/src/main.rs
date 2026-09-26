#[tokio::main(flavor = "current_thread")]
async fn main() {
    // The SDK graph may enable aws-lc alongside this provider's ring TLS clients.
    // rustls cannot infer a process default when both backends are compiled in.
    if rustls::crypto::ring::default_provider()
        .install_default()
        .is_err()
    {
        eprintln!("OpenAI worker could not install the ring TLS crypto provider");
        std::process::exit(1);
    }
    if let Err(error) = openai_provider::serve_multiplex_worker().await {
        eprintln!("OpenAI multiplex worker failed: {error}");
        std::process::exit(1);
    }
}
