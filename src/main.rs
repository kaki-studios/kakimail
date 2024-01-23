use anyhow::*;
use hickory_resolver::config::{ResolverConfig, ResolverOpts};
use tokio::net::TcpListener;

mod smtp;
mod utils;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();
    //NB cant use 25 since its blocked by isp/os???
    let addr = "127.0.0.1:2525";
    //NB no ec2 instance spun up yet, will not work!
    let _domain = "mail.kaki.foo";

    let listener = TcpListener::bind(&addr).await?;
    tracing::info!("listening on: {}", addr);
    let resolver = utils::DnsResolver {
        resolver: hickory_resolver::AsyncResolver::tokio(
            ResolverConfig::default(),
            ResolverOpts::default(),
        ),
    };

    let ip = resolver.resolve_mx("gmail.com").await?;

    tracing::info!("gmails top ip is: {:?}", ip);

    loop {
        let (_stream, addr) = listener.accept().await?;
        tracing::info!("recieved new connection from {:?}", addr);
    }
}
