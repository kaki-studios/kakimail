use std::sync::Arc;

use anyhow::{anyhow, Context, Ok, Result};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    sync::Mutex,
};
use tracing::field::debug;

use crate::database;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd)]
enum IMAPState {
    NotAuthed = 0,
    Authed = 1,
    Selected = 2,
    Logout = 3,
}
struct IMAPStateMachine {
    //add new fields as needed. prolly need TLS stuff later on
    state: IMAPState,
}

impl IMAPStateMachine {
    //eg: "* OK [CAPABILITY STARTTLS AUTH=SCRAM-SHA-256 LOGINDISABLED IMAP4rev2] IMAP4rev2 Service Ready"
    const GREETING: &'static [u8] = b"* OK IMAP4rev2 Service Ready\r\n";
    const _HOLD_YOUR_HORSES: &'static [u8] = &[];

    fn new() -> Self {
        Self {
            state: IMAPState::NotAuthed,
        }
    }
    //weird return type ik, NOTE: inefficient and hacky
    fn handle_imap(&mut self, raw_msg: &str) -> Result<Vec<Vec<u8>>> {
        tracing::trace!("Received {raw_msg} in state {:?}", self.state);
        let mut msg = raw_msg.split_whitespace();
        let tag = msg.next().context("received empty tag")?;
        let command = msg.next().context("received empty command")?.to_lowercase();
        let state = self.state.clone();
        match (command.as_str(), state) {
            //ANY STATE
            ("noop", _) => {
                let value = format!("{} OK NOOP completed\r\n", tag);
                Ok(vec![value.as_bytes().to_vec()])
            }
            ("capability", _) => {
                let value = "* CAPABILITY IMAP4rev1 AUTH=PLAIN\r\n";
                let value2 = format!("{} OK CAPABILITY completed\r\n", tag);
                Ok(vec![value.as_bytes().to_vec(), value2.as_bytes().to_vec()])
            }
            ("logout", _) => {
                let mut resp = Vec::new();
                let untagged = "* BYE IMAP4rev2 Server logging out\r\n".as_bytes().to_vec();
                resp.push(untagged);
                let tagged = format!("{} OK LOGOUT completed\r\n", tag)
                    .as_bytes()
                    .to_vec();
                resp.push(tagged);
                self.state = IMAPState::Logout;
                Ok(resp)
            }
            //NOT AUTHED STATE
            //starttls can be issued at "higher" states too
            ("starttls", x) if x >= IMAPState::NotAuthed => {
                let value = format!("{}, NO starttls not implemented yet\r\n", tag);
                Ok(vec![value.as_bytes().to_vec()])
            }
            ("login", IMAPState::NotAuthed) => {
                let username = msg.next().context("should provide username")?;
                let mut password = msg.next().context("should provice password")?;
                //NOTE: python's imaplib submits passwords enclosed like this: \"password\"
                //so we will need to remove them
                password = &password[1..password.len() - 1];
                if username == std::env::var("USERNAME")? && password == std::env::var("PASSWORD")?
                {
                    let good_msg = format!("{} OK LOGIN COMPLETED\r\n", tag);
                    self.state = IMAPState::Authed;
                    Ok(vec![good_msg.as_bytes().to_vec()])
                } else {
                    let bad_msg = format!("{} NO LOGIN INVALID\r\n", tag);
                    Ok(vec![bad_msg.as_bytes().to_vec()])
                }
            }
            ("authenticate", IMAPState::NotAuthed) => {
                let method = msg
                    .next()
                    .context("should provide auth mechanism")?
                    .to_lowercase();
                if method != "plain" {
                    //not supported
                } else {
                    let _login_encoded = msg.next().context("should provide login info")?;

                    //decode the same way as utils.rs
                }
                //READ: https://datatracker.ietf.org/doc/html/rfc9051#name-authenticate-command
                Err(anyhow!("authenticate not implemented yet!"))
            }
            ("enable", x) if x >= IMAPState::Authed => {
                let response = format!("{} BAD NO EXTENSIONS SUPPORTED", tag);
                Ok(vec![response.as_bytes().to_vec()])
            }
            ("select", IMAPState::Authed) => {
                //TODO
                //need functionality in database.rs
                //see: https://datatracker.ietf.org/doc/html/rfc9051#name-select-command
                Err(anyhow!("not implemented"))
            }
            ("examine", IMAPState::Authed) => {
                //same as select but the mailbox returned is read-only
                Err(anyhow!("not implemented"))
            }
            ("create", IMAPState::Authed) => {
                //TODO
                Err(anyhow!("not implemented"))
            }
            ("delete", IMAPState::Authed) => {
                //TODO
                Err(anyhow!("not implemented"))
            }
            ("rename", IMAPState::Authed) => {
                //TODO
                Err(anyhow!("not implemented"))
            }
            ("subscribe", IMAPState::Authed) => {
                //TODO
                Err(anyhow!("not implemented"))
            }
            ("unsubscribe", IMAPState::Authed) => {
                //TODO
                Err(anyhow!("not implemented"))
            }
            ("list", IMAPState::Authed) => {
                //TODO
                Err(anyhow!("not implemented"))
            }
            ("namespace", IMAPState::Authed) => {
                //TODO
                Err(anyhow!("not implemented"))
            }
            ("status", IMAPState::Authed) => {
                //TODO
                Err(anyhow!("not implemented"))
            }
            ("append", IMAPState::Authed) => {
                //TODO
                Err(anyhow!("not implemented"))
            }
            ("idle", IMAPState::Authed) => {
                //TODO
                Err(anyhow!("not implemented"))
            }
            //MORE
            _ => anyhow::bail!(
                "Unexpected message received in state {:?}: {raw_msg}",
                self.state
            ),
        }
    }
}

pub struct IMAP {
    pub stream: tokio::net::TcpStream,
    state_machine: IMAPStateMachine,
    pub db: Arc<Mutex<database::Client>>,
}

impl IMAP {
    /// Creates a new server from a connected stream
    pub async fn new(stream: tokio::net::TcpStream) -> Result<Self> {
        Ok(Self {
            stream,
            state_machine: IMAPStateMachine::new(),
            db: Arc::new(Mutex::new(database::Client::new().await?)),
        })
    }

    pub async fn serve(mut self) -> Result<()> {
        self.greet().await?;

        let mut buf: [u8; 65536] = [0; 65536];
        loop {
            let n = self.stream.read(&mut buf).await?;

            if n == 0 {
                tracing::info!("Received EOF");
                self.state_machine.handle_imap("logout").ok();
                break;
            }
            let msg = std::str::from_utf8(&buf[0..n])?;
            let responses = self.state_machine.handle_imap(msg)?;
            for response in responses {
                self.stream.write_all(&response).await?;
            }
            if self.state_machine.state == IMAPState::Logout {
                break;
            }
            // if response != SMTPStateMachine::HOLD_YOUR_HORSES {

            // } else {
            // tracing::debug!("Not responding, awaiting for more data");
            // }
            // if response == SMTPStateMachine::KTHXBYE {
            //     break;
            // }
        }
        Ok(())
    }
    async fn greet(&mut self) -> Result<()> {
        self.stream
            .write_all(IMAPStateMachine::GREETING)
            .await
            .map_err(|e| e.into())
    }
}
