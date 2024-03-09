use anyhow::*;
use core::result::Result::Ok;
use dotenv::dotenv;
use tokio::net::TcpListener;
use tracing::Level;
use tracing_subscriber::filter;
use tracing_subscriber::fmt;
use tracing_subscriber::prelude::*;

mod database;
mod imap;
mod smtp_common;
mod smtp_incoming;
mod smtp_outgoing;
mod utils;

#[tokio::main]
async fn main() -> Result<()> {
    // dotenv().ok();
    dotenv()?;
    tracing_subscriber::registry()
        .with(fmt::layer().with_filter(filter::LevelFilter::from_level(Level::TRACE)))
        .init();
    // tracing_subscriber::fmt::init();
    let mut args = std::env::args();

    let smtp_addr = args.nth(1).unwrap_or("127.0.0.1".to_string());
    let smtp_port = args.next().unwrap_or("25".to_string());
    let smtp_subm = args.next().unwrap_or("587".to_string());
    let imap_port = args.next().unwrap_or("143".to_string());
    tracing::info!("{:?}", (&smtp_addr, &smtp_port, &smtp_subm, &imap_port));

    let domain = &args.next().unwrap_or("smtp.kaki.foo".to_string());

    let incoming_listener = TcpListener::bind(format!("{smtp_addr}:{smtp_port}")).await?;
    let outgoing_listener = TcpListener::bind(format!("{smtp_addr}:{smtp_subm}")).await?;
    let imap_listener = TcpListener::bind(format!("{smtp_addr}:{imap_port}")).await?;
    tracing::info!("listening on: {}", smtp_addr);
    tracing::info!("smtp port is: {}", smtp_port);
    tracing::info!("submission port is: {}", smtp_subm);
    tracing::info!("imap port is: {}", imap_port);
    tracing::info!("smtp server for {domain} started!");

    //main server loop
    loop {
        tokio::select! {
            Ok((incoming_stream, incoming_addr)) = incoming_listener.accept() => {
                tracing::info!("recieved incoming connection from {}", incoming_addr);
                tokio::task::LocalSet::new()
                    .run_until(async move {
                        let smtp = smtp_incoming::SmtpIncoming::new(domain.to_string(), incoming_stream).await?;
                        smtp.serve().await
                    })
                    .await
                    .ok();
            }
            Ok((outgoing_stream, outgoing_addr)) = outgoing_listener.accept() => {
                tracing::info!("recieved outgoing connection from {}", outgoing_addr);
                tokio::task::LocalSet::new()
                    .run_until(async move {
                        let smtp = smtp_outgoing::SmtpOutgoing::new(domain.to_string(), outgoing_stream).await?;
                        smtp.serve().await
                    })
                    .await
                    .ok();
            }
            Ok((imap_stream, imap_addr)) = imap_listener.accept() => {
                tracing::info!("recieved imap connection from {}", imap_addr);
                tokio::task::LocalSet::new()
                    .run_until(async move {
                        let imap = imap::IMAP::new(imap_stream).await?;
                        imap.serve().await
                    })
                    .await
                    .ok();
            }
        }
    }
}
