use anyhow::*;
use core::result::Result::Ok;
use tokio::net::TcpListener;

mod database;
mod smtp_incoming;
mod smtp_outgoing;
mod utils;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let smtp_addr = std::env::args().nth(1).unwrap_or("127.0.0.1".to_string());

    let domain = &std::env::args()
        .nth(2)
        .unwrap_or("smtp.kaki.foo".to_string());

    let incoming_listener = TcpListener::bind(format!("{smtp_addr}:25")).await?;
    let outgoing_listener = TcpListener::bind(format!("{smtp_addr}:587")).await?;
    tracing::info!("listening on: {}", smtp_addr);
    tracing::info!("smtp server for {domain} started!");

    // let resolver = utils::DnsResolver::default_new();
    // let _ip = resolver.resolve_mx("gmail.com").await?;

    //main server loop
    loop {
        tokio::select! {
            Ok((incoming_stream, incoming_addr)) = incoming_listener.accept() => {
                tracing::info!("recieved incoming connection from {}", incoming_addr);
                tokio::task::LocalSet::new()
                    .run_until(async move {
                        let smtp = smtp_incoming::SmtpIncoming::new(domain, incoming_stream).await?;
                        smtp.serve().await
                    })
                    .await
                    .ok();
            }
            Ok((outgoing_stream, outgoing_addr)) = outgoing_listener.accept() => {
                tracing::info!("recieved outgoing connection from {}", outgoing_addr);
                tokio::task::LocalSet::new()
                    .run_until(async move {
                        let smtp = smtp_outgoing::SmtpOutgoing::new(domain, outgoing_stream).await?;
                        smtp.serve().await
                    })
                    .await
                    .ok();

            }
        }
    }
}
