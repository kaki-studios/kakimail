use std::sync::Arc;

use crate::smtp_common::*;
use anyhow::*;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    sync::Mutex,
};

use crate::database;

pub struct SmtpIncoming {
    pub stream: tokio::net::TcpStream,
    pub state_machine: StateMachine,
    pub db: Arc<Mutex<database::Client>>,
}

impl SmtpIncoming {
    /// Creates a new server from a connected stream
    pub async fn new(domain: impl AsRef<str>, stream: tokio::net::TcpStream) -> Result<Self> {
        Ok(Self {
            stream,
            state_machine: StateMachine::new(domain, false),
            db: Arc::new(Mutex::new(database::Client::new().await?)),
        })
    }

    /// Runs the server loop, accepting and handling SMTP commands
    pub async fn serve(mut self) -> Result<()> {
        self.greet().await?;

        // let mut buf = vec![0; 65536];
        let mut buf: [u8; 65536] = [0; 65536];
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
                tracing::debug!("Not responding, awaiting for more data");
            }
            if response == StateMachine::KTHXBYE {
                break;
            }
        }
        match self.state_machine.state {
            State::Received(mail) => {
                tracing::info!("got mail!");
                self.db.lock().await.replicate(mail, false).await?;
            }
            State::ReceivingData(mail) => {
                tracing::info!("Received EOF before receiving QUIT");
                self.db.lock().await.replicate(mail, false).await?;
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
