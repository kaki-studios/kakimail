use std::sync::Arc;

use crate::database;
use crate::smtp_common::Mail;
use crate::smtp_common::SMTPState;
use crate::smtp_common::SMTPStateMachine;
use crate::utils;
use anyhow::Context;
use anyhow::Result;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::Mutex;

pub struct SmtpOutgoing {
    pub stream: tokio::net::TcpStream,
    //should add message queue when using seriously
    pub state_machine: SMTPStateMachine,
    pub db: Arc<Mutex<database::DBClient>>,
}

impl SmtpOutgoing {
    /// Creates a new server from a connected stream
    pub async fn new(domain: impl AsRef<str>, stream: tokio::net::TcpStream) -> Result<Self> {
        Ok(Self {
            stream,
            state_machine: SMTPStateMachine::new(domain, true),
            db: Arc::new(Mutex::new(database::DBClient::new().await?)),
        })
    }
    pub async fn serve(mut self) -> Result<()> {
        self.greet().await?;
        tracing::info!("greeted!");
        // let mut buf = vec![0; 65536];
        let mut buf: &mut [u8] = &mut [0; 65536];
        loop {
            let n = self.stream.read(&mut buf).await?;
            if n == 0 {
                tracing::info!("Received EOF");
                self.state_machine
                    //not handling auth so don't need handle_smtp_outgoing()
                    .handle_smtp_incoming("quit")
                    .ok();
                break;
            }
            let msg = std::str::from_utf8(&buf[0..n])?;
            dbg!(&msg);
            let response = self
                .state_machine
                .handle_smtp_outgoing(msg, self.db.clone())
                .await?;
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
            SMTPState::Received(ref mail, id) => {
                //send mail, everything was succesful!
                self.handle_mail(mail, id.context("should exist")?).await?;
            }
            SMTPState::ReceivingData(ref mail, x) => {
                tracing::info!("Received EOF before receiving QUIT");
                tracing::info!("{:?}", (mail, x));
                self.handle_mail(mail, x.context("should exist")?).await?;
            }
            _ => {}
        }
        Ok(())
    }
    async fn handle_mail(&self, mail: &Mail, id: i32) -> Result<()> {
        SmtpOutgoing::send_mail(&mail).await.map_err(|e| {
            tracing::error!("{:?}", e);
            e
        })?;
        let id = self.db.lock().await.get_mailbox_id(id, "INBOX").await?;
        self.db
            .lock()
            .await
            .replicate(mail.clone(), id, None)
            .await?;
        Ok(())
    }

    /// Sends the initial SMTP greeting
    async fn greet(&mut self) -> Result<()> {
        self.stream
            .write_all(SMTPStateMachine::OH_HAI)
            .await
            .map_err(|e| {
                tracing::error!("error greeting: {}", e);
                e.into()
            })
    }
    async fn send_mail(mail: &crate::smtp_common::Mail) -> Result<()> {
        let resolver = utils::DnsResolver::default_new();
        for rcpt in &mail.to {
            if let Some((_, domain)) = rcpt.split_once("@") {
                let domain = domain
                    .strip_suffix(">")
                    .context("should be formatted inside angle brackets")?; //NOTE: hacky
                let ip = resolver.resolve_mx(domain).await?;
                // let ip = "127.0.0.1";
                //BIG TODO: this will timeout on port 25 unless you request to unblock port 25
                let mut connection = TcpStream::connect((ip, 25)).await?;
                tracing::debug!("connection succesful");
                // let mut buf = vec![0; 65536];
                let mut buf: [u8; 65536] = [0; 65536];
                let commands = Self::gen_commands(&mail);
                let n = connection.read(&mut buf).await?;
                let string = std::str::from_utf8(&buf[0..n])?;
                tracing::debug!("greeting: {string}");
                for cmd in commands {
                    connection.write_all(cmd.as_bytes()).await?;
                    tracing::debug!("wrote: {cmd}");
                    let n = connection.read(&mut buf).await?;
                    let string = std::str::from_utf8(&buf[0..n])?;
                    tracing::debug!("read: {string}");
                    let statuscode = &string[..3];
                    match statuscode
                        .chars()
                        .next()
                        .ok_or(anyhow::anyhow!("bad statuscode"))?
                    {
                        '2' | '3' => {
                            //2yz (Positive Completion Reply): The requested action has been successfully completed.
                            //3yz (Positive Intermediate Reply): The command has been accepted, but the requested action
                            //is being held in abeyance, pending receipt of further information.

                            //everything was ok
                            //3yx codes usually appear like: "354 End data with <CR><LF>.<CR><LF>\n"
                            //so don't do anything
                            tracing::info!("{statuscode}");
                        }
                        '4' => {
                            //4yz (Transient Negative Completion Reply): The command was not accepted, and the requested action did not occur.
                            //However, the error condition is temporary, and the action may be requested again.
                            tracing::warn!("got a 4yx statuscode: {statuscode}, trying again");
                            // try again?
                            connection.write_all(cmd.as_bytes()).await?;
                        }
                        '5' => {
                            //5yz (Permanent Negative Completion Reply): The command was not accepted and the requested action did not occur.
                            //The SMTP client SHOULD NOT repeat the exact request (in the same sequence).
                            tracing::warn!("got a 5yx statuscode: {statuscode}, ending connection");
                            //quit?
                            connection.write_all("quit\r\n".as_bytes()).await?;
                        }
                        _ => {
                            tracing::warn!("invalid statuscode: {statuscode}");
                        }
                    }
                }
                connection.write_all("quit\r\n".as_bytes()).await?;
                tracing::info!("all succesful!");
            }
        }
        Ok(())
    }
    fn gen_commands(mail: &crate::smtp_common::Mail) -> Vec<String> {
        let mut commands: Vec<String> = Vec::new();
        let domain = std::env::args()
            .nth(4)
            .unwrap_or("smtp.kaki.foo".to_string());
        commands.push(format!("ehlo {domain}\r\n"));
        commands.push(format!("mail FROM:<{}>\r\n", mail.from));
        for rcpt in &mail.to {
            commands.push(format!("rcpt TO:<{rcpt}>\r\n"));
        }
        commands.push("data\r\n".to_string());
        if mail.data.ends_with("\r\n.\r\n") {
            commands.push(mail.data.clone());
        } else {
            commands.push(format!("{}\r\n.\r\n", mail.data));
        }
        //don't push "quit", it will be seperate

        commands
    }
}
