use std::sync::Arc;

use crate::{smtp_common::*, tls::StreamType};
use anyhow::*;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    sync::{mpsc::Sender, Mutex},
};

use crate::database;

pub struct SmtpIncoming {
    // pub stream: tokio::net::TcpStream,
    pub stream: StreamType,
    pub state_machine: SMTPStateMachine,
    pub db: Arc<Mutex<database::DBClient>>,
    pub domain: String,
    pub acceptor: tokio_rustls::TlsAcceptor,
}

impl SmtpIncoming {
    /// Creates a new server from a connected stream
    pub async fn new(
        domain: String,
        stream: tokio::net::TcpStream,
        domain_stripped: String,
        tx: Sender<String>,
        implicit_tls: bool,
        acceptor: tokio_rustls::TlsAcceptor,
    ) -> Result<Self> {
        let stream_type = if !implicit_tls {
            StreamType::Plain(stream)
        } else {
            let tls_stream = acceptor.accept(stream).await?;
            StreamType::Tls(tls_stream)
        };
        Ok(Self {
            stream: stream_type,
            state_machine: SMTPStateMachine::new(domain.clone(), false),
            db: Arc::new(Mutex::new(database::DBClient::new(tx).await?)),
            domain: domain_stripped,
            acceptor,
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
                if response == SMTPStateMachine::READY_FOR_ENCRYPTION {
                    self.stream = self.stream.upgrade_to_tls(self.acceptor.clone()).await?;
                }
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
                self.store_mail(mail).await;
            }
            SMTPState::ReceivingData(ref mail, _) => {
                tracing::info!("Received EOF before receiving QUIT");
                self.store_mail(mail).await;
            }
            _ => {}
        }
        Ok(())
    }
    ///saves the mail in the recipients' INBOXes
    async fn store_mail(&self, mail: &Mail) {
        let db = self.db.lock().await;
        for i in &mail.to {
            //go from <user@domain.com> to user@domain.com. strip the angle brackets
            let i = &i[1..i.len() - 1];
            let mut parts = i.split("@").into_iter();
            let Some(user) = parts.next() else {
                tracing::warn!("no user: {i}");
                continue;
            };
            let Some(domain) = parts.next() else {
                tracing::warn!("no domain: {i}");
                continue;
            };
            dbg!(user, domain);
            if domain != self.domain {
                tracing::warn!("invalid domain: {i}");
                continue;
            }
            let Some(user_id) = db.get_user_id(user).await else {
                //TODO: make this check earlier, while doing smtp so client can know
                tracing::warn!("invalid user: {i}");
                continue;
            };
            let Some(m_id) = db.get_mailbox_id(user_id, "INBOX").await.ok() else {
                tracing::warn!("invalid inbox for user: {i}");
                continue;
            };
            db.replicate(mail.clone(), m_id, None)
                .await
                .map_err(|e| {
                    tracing::error!("{}", e);
                    e
                })
                .ok();
        }
    }

    /// Sends the initial SMTP greeting
    async fn greet(&mut self) -> Result<()> {
        self.stream
            .write_all(SMTPStateMachine::OH_HAI)
            .await
            .map_err(|e| e.into())
    }
}
