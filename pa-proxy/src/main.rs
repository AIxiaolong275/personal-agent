#[tokio::main]
async fn main() {
    pa_proxy::server::serve().await;
}
