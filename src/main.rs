use anyhow::*;
use tokio::net::TcpListener;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();
    //NB cant use 25 since its blocked by isp/os???
    let addr = "127.0.0.1:2525";

    let listener = TcpListener::bind(&addr).await?;
    tracing::info!("listening on: {}", addr);
    loop {
        let (_stream, addr) = listener.accept().await?;
        tracing::info!("recieved new connection from {:?}", addr);
    }
}
