use runtime_extension_sdk::serve;
use wechat_chat_channels::{dispatch, HttpClient};

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = std::rc::Rc::new(HttpClient::new()?);
    serve(move |request, _| {
        let client = client.clone();
        async move { dispatch(&client, request).await }
    })
    .await?;
    Ok(())
}
