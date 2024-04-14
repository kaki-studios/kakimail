use anyhow::*;
use core::result::Result::Ok;
use dotenv::dotenv;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::Mutex;
use tokio_rustls::TlsAcceptor;

mod database;
mod imap;
mod imap_op;
mod smtp_common;
mod smtp_incoming;
mod smtp_outgoing;
mod tls;
mod utils;

#[tokio::main]
async fn main() -> Result<()> {
    dotenv()?;

    tracing_subscriber::fmt::init();
    let mut args = std::env::args();

    let smtp_addr = args.nth(1).unwrap_or("127.0.0.1".to_string());
    let smtp_port = args.next().unwrap_or("25".to_string());
    let smtp_subm = args.next().unwrap_or("587".to_string());
    let imap_port = args.next().unwrap_or("143".to_string());

    let domain = &args.next().unwrap_or("smtp.kaki.foo".to_string());

    //go from smtp.kaki.foo to kaki.foo
    let domain_stripped = &domain.split(".").collect::<Vec<&str>>()[1..].join(".");
    tracing::info!("requesting certs...");
    let client = reqwest::Client::new();
    let mut resp = client
        .post(format!(
            "https://porkbun.com/api/json/v3/ssl/retrieve/{}",
            domain_stripped
        ))
        .body(format!(
            //TODO don't hardcode the json, looks ugly
            "
            {{
                \"secretapikey\": \"{}\",
                \"apikey\": \"{}\"
            }}
            ",
            std::env::var("PORKBUN_SECRET_API_KEY")?,
            std::env::var("PORKBUN_API_KEY")?
        ))
        .send()
        .await?
        .json::<HashMap<String, String>>()
        .await?;
    let cert_chain = resp
        .get_mut("certificatechain")
        .context("should provide certchain")?;
    let certs = rustls_pemfile::certs(&mut std::io::Cursor::new(cert_chain.clone()))
        .flatten()
        .collect::<Vec<_>>();
    let key_resp = resp
        .get("privatekey")
        .context("should provide private key")?;
    let key = rustls_pemfile::private_key(&mut std::io::Cursor::new(key_resp.clone()))?
        .context("should be a valid key")?;
    let config = tokio_rustls::rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key.into())?;
    let acceptor = &TlsAcceptor::from(Arc::new(config));
    tracing::debug!("acceptor ready");

    let incoming_listener = TcpListener::bind(format!("{smtp_addr}:{smtp_port}")).await?;
    let outgoing_listener = TcpListener::bind(format!("{smtp_addr}:{smtp_subm}")).await?;
    let imap_listener = TcpListener::bind(format!("{smtp_addr}:{imap_port}")).await?;
    //TODO implicit imap tls listener!
    tracing::info!("listening on: {}", smtp_addr);
    tracing::info!("smtp port is: {}", smtp_port);
    tracing::info!("submission port is: {}", smtp_subm);
    tracing::info!("imap port is: {}", imap_port);
    tracing::info!("smtp server for {domain} started!");
    let (tx, rx) = tokio::sync::mpsc::channel::<String>(128);

    let new_rx = Arc::new(Mutex::new(rx));
    let new_tx = &tx;
    //main server loop
    loop {
        let loop_rx = new_rx.clone();
        tokio::select! {
            Ok((incoming_stream, incoming_addr)) = incoming_listener.accept() => {
                tracing::info!("recieved incoming connection from {}", incoming_addr);
                tokio::task::LocalSet::new()
                    .run_until(async move {
                        let smtp = smtp_incoming::SmtpIncoming::new(domain.to_string(), incoming_stream, domain_stripped.to_string(),
                            new_tx.clone(), false ,acceptor.clone()).await?;
                        smtp.serve().await
                    })
                    .await
                    .ok();
            }
            Ok((outgoing_stream, outgoing_addr)) = outgoing_listener.accept() => {
                tracing::info!("recieved outgoing connection from {}", outgoing_addr);
                tokio::task::LocalSet::new()
                    .run_until(async move {
                        let smtp = smtp_outgoing::SmtpOutgoing::new(domain.to_string(), outgoing_stream, new_tx.clone(), false,
                            acceptor.clone()).await?;
                        smtp.serve().await
                    })
                    .await
                    .ok();
            }
            Ok((imap_stream, imap_addr)) = imap_listener.accept() => {
                tracing::info!("recieved imap connection from {}", imap_addr);
                tokio::task::LocalSet::new()
                    .run_until(async move {
                        let imap = imap::IMAP::new(imap_stream,acceptor.clone(),false, new_tx.clone(),loop_rx).await?;
                        imap.serve().await
                    })
                    .await
                    .ok();
            }
        }
    }
}
