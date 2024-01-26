use anyhow::*;
use tokio::net::TcpListener;

mod database;
mod smtp_incoming;
mod smtp_outgoing;
mod utils;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();
    //NB cant use 25 since its blocked probably by isp???
    let smtp_addr = "127.0.0.1:2525";
    //NB ec2 instance not set up yet, will not work!
    let _domain = "mail.kaki.foo";

    let smtp_listener = TcpListener::bind(&smtp_addr).await?;
    tracing::info!("listening on: {}", smtp_addr);
    let resolver = utils::DnsResolver::default_new();

    let _ip = resolver.resolve_mx("gmail.com").await?;
    //main server loop
    loop {
        let (_stream, addr) = smtp_listener.accept().await?;
        tracing::info!("recieved new connection from {:?}", addr);
    }
}
