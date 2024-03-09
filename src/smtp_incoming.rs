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
    pub state_machine: SMTPStateMachine,
    pub db: Arc<Mutex<database::Client>>,
    pub domain: String,
}

impl SmtpIncoming {
    /// Creates a new server from a connected stream
    pub async fn new(domain: String, stream: tokio::net::TcpStream) -> Result<Self> {
        Ok(Self {
            stream,
            state_machine: SMTPStateMachine::new(domain.clone(), false),
            db: Arc::new(Mutex::new(database::Client::new().await?)),
            domain,
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
                self.state_machine.handle_smtp_incoming("quit").ok();
                break;
            }
            let msg = std::str::from_utf8(&buf[0..n])?;
            let response = self.state_machine.handle_smtp_incoming(msg)?;
            if response != SMTPStateMachine::HOLD_YOUR_HORSES {
                self.stream.write_all(response).await?;
            } else {
                tracing::debug!("Not responding, awaiting for more data");
            }
            if response == SMTPStateMachine::KTHXBYE {
                break;
            }
        }
        match self.state_machine.state {
            SMTPState::Received(ref mail, _) => {
                tracing::info!("got mail!");
                self.store_mail(mail).await?;
            }
            SMTPState::ReceivingData(ref mail, _) => {
                tracing::info!("Received EOF before receiving QUIT");
                self.store_mail(mail).await?;
            }
            _ => {}
        }
        Ok(())
    }

    async fn store_mail(&self, mail: &Mail) -> Result<()> {
        // self.db.lock().await.replicate(mail, 0).await.map_err(|e| {
        //     tracing::error!("{:?}", e);
        //     e
        // })?;
        for i in &mail.to {
            //go from <user@domain.com> to user@domain.com. strip the angle brackets
            let i = &i[1..i.len() - 1];
            let mut parts = i.split("@").into_iter();
            let Some(user) = parts.next() else {
                continue;
            };
            let Some(domain) = parts.next() else {
                continue;
            };
            dbg!(user, domain);
            if domain != self.domain {
                continue;
            }

            //TODO
            //db.get_user_id()!!!
            //if domail == our domain, replicate it. if no user found, continue.
        }
        Ok(())
    }

    /// Sends the initial SMTP greeting
    async fn greet(&mut self) -> Result<()> {
        self.stream
            .write_all(SMTPStateMachine::OH_HAI)
            .await
            .map_err(|e| e.into())
    }
}
