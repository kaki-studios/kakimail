// use std::sync::Arc;

// use crate::utils::Mail;
use crate::smtp_common::State;
use crate::smtp_common::StateMachine;
use crate::utils;
use anyhow::Result;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    // sync::Mutex,
};
// use tokio::sync::Mutex;

// use crate::smtp_incoming::StateMachine;

//hehe
#[allow(unused)]
pub struct SmtpOutgoing {
    stream: tokio::net::TcpStream,
    //tcp stream
    //also need something like this...
    // message_queue: Arc<Mutex<Vec<Mail>>>,
    state_machine: StateMachine,
}

impl SmtpOutgoing {
    #[allow(unused)]
    pub async fn new(domain: impl AsRef<str>, stream: tokio::net::TcpStream) -> Result<Self> {
        Ok(Self {
            stream,
            state_machine: StateMachine::new(domain, true),
            // message_queue: Arc::new(Mutex::new(Vec::new())),
        })
    }
    pub async fn serve(mut self) -> Result<()> {
        self.greet().await?;
        let mut buf = vec![0; 65536];
        loop {
            let n = self.stream.read(&mut buf).await?;
            if n == 0 {
                tracing::info!("Received EOF");
                self.state_machine.handle_smtp("quit").ok();
                break;
            }
            let msg = std::str::from_utf8(&buf[0..n])?;
            let response = self.state_machine.handle_smtp(msg)?;
            if response != StateMachine::HOLD_YOUR_HORSES {
                self.stream.write_all(response).await?;
            } else {
                tracing::debug!("Not responding, awaiting more data");
            }
            if response == StateMachine::KTHXBYE {
                break;
            }
        }
        //TODO: require auth with mail submission (do the logic in ./smtp_common.rs)
        match self.state_machine.state {
            State::Received(mail) => {
                for rcpt in mail.to {
                    if let Some((_, domain)) = rcpt.split_once("@") {
                        let domain = domain
                            .strip_suffix(">")
                            .expect("emails to be formatted inside angle brackets"); //hacky
                        let resolver = utils::DnsResolver::default_new();
                        let ip = resolver.resolve_mx(domain).await?;
                        //TODO: establish connection on port 25 and send the appropriate smtp
                        //commands (maybe need a new state_machine???)
                    }
                }
            }
            State::ReceivingData(mail) => {
                tracing::info!("Received EOF before receiving QUIT");
                tracing::info!("{:?}", mail);
            }
            _ => {}
        }
        Ok(())
    }
    /// Sends the initial SMTP greeting
    async fn greet(&mut self) -> Result<()> {
        self.stream
            .write_all(StateMachine::OH_HAI)
            .await
            .map_err(|e| e.into())
    }
}
