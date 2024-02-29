use std::sync::Arc;

use anyhow::{anyhow, Context, Ok, Result};
use base64::Engine;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    sync::Mutex,
};

use crate::database;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd)]
enum IMAPState {
    NotAuthed,
    WaitingForAuth(String),
    Authed,
    ///false if read only
    Selected(bool),
    Logout,
}
pub struct IMAP {
    //add new fields as needed. prolly need TLS stuff later on
    state: IMAPState,
    stream: tokio::net::TcpStream,
    db: Arc<Mutex<database::Client>>,
}

impl IMAP {
    //eg: "* OK [CAPABILITY STARTTLS AUTH=SCRAM-SHA-256 LOGINDISABLED IMAP4rev2] IMAP4rev2 Service Ready"
    const GREETING: &'static [u8] = b"* OK IMAP4rev2 Service Ready\r\n";
    const _HOLD_YOUR_HORSES: &'static [u8] = &[];
    const FLAGS: &'static [u8] = b"* FLAGS (\\Answered \\Flagged \\Deleted \\Seen \\Draft)\r\n";
    const PERMANENT_FLAGS: &'static [u8] = b"* OK [PERMANENTFLAGS (\\Deleted \\Seen \\*)]\r\n";
    const NO_PERMANENT_FLAGS: &'static [u8] =
        b"* OK [PERMANENTFLAGS ()] No permanent flags permitted\r\n";
    const LIST_CMD: &'static [u8] = b"* LIST () \"/\" INBOX\r\n";

    /// Creates a new server from a connected stream
    pub async fn new(stream: tokio::net::TcpStream) -> Result<Self> {
        Ok(Self {
            stream,
            state: IMAPState::NotAuthed,
            db: Arc::new(Mutex::new(database::Client::new().await?)),
        })
    }
    //weird return type ik, NOTE: inefficient and hacky
    async fn handle_imap(&mut self, raw_msg: &str) -> Result<Vec<Vec<u8>>> {
        dbg!(Self::FLAGS);
        tracing::trace!("Received {raw_msg} in state {:?}", self.state);
        if let IMAPState::WaitingForAuth(tag) = &self.state.clone() {
            return match crate::utils::DECODER.decode(raw_msg) {
                Err(_) => Ok(vec![format!("{} BAD INVALID BASE64", tag)
                    .as_bytes()
                    .to_vec()]),
                Result::Ok(decoded) => {
                    if std::str::from_utf8(&decoded)?
                        == &format!(
                            "\0{}\0{}",
                            std::env::var("USERNAME")?,
                            std::env::var("PASSWORD")?
                        )
                    {
                        self.state = IMAPState::Authed;
                        Ok(vec![format!("{} OK Success\r\n", tag).as_bytes().to_vec()])
                    } else {
                        self.state = IMAPState::NotAuthed;
                        Ok(vec![format!("{} BAD Invalid Credentials\r\n", tag)
                            .as_bytes()
                            .to_vec()])
                    }
                }
            };
        }
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
                let value = "* CAPABILITY IMAP4rev1 IMAP4rev2 AUTH=PLAIN\r\n";
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
                //NOTE: this approach does't support passwords with spaces, but I think that's ok
                //for now
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
                    Ok(vec![format!(
                        "{} BAD Unsupported Authentication Mechanism",
                        tag
                    )
                    .as_bytes()
                    .to_vec()])
                    //not supported
                } else {
                    match msg.next() {
                        None => {
                            //login will be in next message
                            self.state = IMAPState::WaitingForAuth(tag.to_string());
                            Ok(vec!["+\r\n".as_bytes().to_vec()])
                        }
                        Some(encoded) => match crate::utils::DECODER.decode(encoded) {
                            Err(_) => Ok(vec![format!("{} BAD INVALID BASE64\r\n", tag)
                                .as_bytes()
                                .to_vec()]),
                            Result::Ok(decoded) => {
                                if std::str::from_utf8(&decoded)?
                                    == &format!(
                                        "\0{}\0{}",
                                        std::env::var("USERNAME")?,
                                        std::env::var("PASSWORD")?
                                    )
                                {
                                    self.state = IMAPState::Authed;
                                    Ok(vec![format!("{} OK Success\r\n", tag).as_bytes().to_vec()])
                                } else {
                                    Ok(vec![format!("{} BAD Invalid Credentials\r\n", tag)
                                        .as_bytes()
                                        .to_vec()])
                                }
                            }
                        },
                    }
                }
            }
            ("enable", x) if x >= IMAPState::Authed => {
                let response = format!("{} BAD NO EXTENSIONS SUPPORTED\r\n", tag);
                Ok(vec![response.as_bytes().to_vec()])
            }
            (x, IMAPState::Authed) if x == "select" || x == "examine" => {
                let mailbox = match msg.next().context("should provide mailbox name") {
                    Err(_) => {
                        return Ok(vec![format!("{} BAD missing arguments\r\n", tag)
                            .as_bytes()
                            .to_vec()])
                    }
                    Result::Ok(a) => a,
                };
                //NOTE: only one mailbox for now idk
                if mailbox != "INBOX" {
                    return Ok(vec![format!("{} NO no such mailbox\r\n", tag)
                        .as_bytes()
                        .to_vec()]);
                }
                let unix_time = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .context("Time shouldn't go backwards")?;
                let seconds: u32 = unix_time.as_secs().try_into()?;

                let uid_validity = format!("* OK [UIDVALIDITY {}]\r\n", seconds)
                    .as_bytes()
                    .to_vec();

                let db = self.db.lock().await;

                let count = db.mail_count().await.context("mail_count failed")?;
                let count_string = format!("* {} EXISTS\r\n", count).as_bytes().to_vec();

                let expected_uid = db.latest_uid().await.unwrap_or(0) + 1;
                let expected_uid_string = format!("* OK [UIDNEXT {}]\r\n", expected_uid)
                    .as_bytes()
                    .to_vec();
                let final_tagged = if x == "select" {
                    format!("{} OK [READ-WRITE] SELECT completed\r\n", tag)
                        .as_bytes()
                        .to_vec()
                } else {
                    format!("{} OK [READ-ONLY] EXAMINE COMPLETED\r\n", tag)
                        .as_bytes()
                        .to_vec()
                };
                let permanent_flags = if x == "select" {
                    Self::PERMANENT_FLAGS
                } else {
                    Self::NO_PERMANENT_FLAGS
                };
                let response = vec![
                    count_string,
                    uid_validity,
                    expected_uid_string,
                    Self::FLAGS.to_vec(),
                    permanent_flags.to_vec(),
                    Self::LIST_CMD.to_vec(),
                    final_tagged,
                ];
                if x == "select" {
                    self.state = IMAPState::Selected(true);
                } else {
                    self.state = IMAPState::Selected(false)
                }
                Ok(response)
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
    pub async fn serve(mut self) -> Result<()> {
        self.greet().await?;
        let mut buf: [u8; 65536] = [0; 65536];
        loop {
            let n = self.stream.read(&mut buf).await?;

            if n == 0 {
                tracing::info!("Received EOF");
                self.handle_imap("logout").await.ok();
                break;
            }
            let msg = std::str::from_utf8(&buf[0..n])?;
            let responses = match self.handle_imap(msg).await {
                Result::Ok(t) => t,
                Err(e) => {
                    tracing::error!("ERROR IN IMAP state machine: \"{:?}\", continuing...", e);
                    vec![]
                }
            };
            for response in responses {
                self.stream.write_all(&response).await?;
            }
            if self.state == IMAPState::Logout {
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
            .write_all(IMAP::GREETING)
            .await
            .map_err(|e| e.into())
    }
}
