use std::sync::Arc;

// use crate::utils::Mail;
use crate::database;
use crate::smtp_common::State;
use crate::smtp_common::StateMachine;
use crate::utils;
use anyhow::Result;
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    // sync::Mutex,
};
// use tokio::sync::Mutex;

// use crate::smtp_incoming::StateMachine;

//hehe
#[allow(unused)]
pub struct SmtpOutgoing {
    pub stream: tokio::net::TcpStream,
    // message_queue: Arc<Mutex<Vec<Mail>>>,
    pub state_machine: StateMachine,
    pub db: Arc<Mutex<database::Client>>,
}

impl SmtpOutgoing {
    #[allow(unused)]
    pub async fn new(domain: impl AsRef<str>, stream: tokio::net::TcpStream) -> Result<Self> {
        Ok(Self {
            stream,
            state_machine: StateMachine::new(domain, true),
            db: Arc::new(Mutex::new(database::Client::new().await?)),
        })
    }
    pub async fn serve(mut self) -> Result<()> {
        self.greet().await?;
        // let mut buf = vec![0; 65536];
        let mut buf: &mut [u8] = &mut [0; 65536];
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
                //send mail, everything was succesful!
                SmtpOutgoing::send_mail(mail.clone()).await?;
                self.db.lock().await.replicate(mail, true).await?;
            }
            State::ReceivingData(mail) => {
                //TODO: should probably still send mail idk
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
    async fn send_mail(mail: crate::smtp_common::Mail) -> Result<()> {
        let resolver = utils::DnsResolver::default_new();
        for rcpt in &mail.to {
            if let Some((_, domain)) = rcpt.split_once("@") {
                let domain = domain
                    .strip_suffix(">") //NOTE: hacky
                    .expect("emails to be formatted inside angle brackets"); //hacky
                let _ip = resolver.resolve_mx(domain).await?;
                //NOTE: here we are sending email to ourselves so that we don't get blacklisted or
                //something else
                let ip = "127.0.0.1";
                let port = "7779";
                //our own port
                //BIG NOTE: this will time out on port 25 unless you request to unblock port 25
                let mut connection = TcpStream::connect(format!("{ip}:{port}")).await?;
                tracing::info!("connection succesful");
                // let mut buf = vec![0; 65536];
                let mut buf: &mut [u8] = &mut [0; 65536];
                let commands = Self::gen_commands(&mail);
                let n = connection.read(&mut buf).await?;
                let string = std::str::from_utf8(&buf[0..n])?;
                tracing::info!("greeting: {string}");
                for cmd in commands {
                    connection.write_all(cmd.as_bytes()).await?;
                    tracing::info!("wrote: {cmd}");
                    let n = connection.read(&mut buf).await?;
                    let string = std::str::from_utf8(&buf[0..n])?;
                    tracing::info!("read: {string}");
                    let statuscode = &string[..3];
                    if statuscode != "250" {
                        if statuscode == "421" {
                            //close connection
                            tracing::warn!("got statuscode 421");
                        } else {
                            //reset connection
                            tracing::warn!("got statuscode {statuscode}");
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
