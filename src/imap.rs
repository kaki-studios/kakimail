use std::{sync::Arc, u8};

use anyhow::{anyhow, Context, Ok, Result};
use base64::Engine;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    sync::Mutex,
};

use crate::{
    database::{self, IMAPFlags},
    utils,
};

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd)]
enum IMAPState {
    NotAuthed,
    WaitingForAuth(String),
    ///userid
    Authed(i32),
    Selected(SelectedState),
    Logout,
}
#[derive(PartialEq, PartialOrd, Eq, Debug, Clone)]
struct SelectedState {
    read_only: bool,
    user_id: i32,
    mailbox_id: i32,
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
    ///shouldn't exist in the future
    const NAMESPACE: &'static [u8] = b"* NAMESPACE ((\"\" \"/\")) NIL NIL\r\n";
    const NO_PERMANENT_FLAGS: &'static [u8] =
        b"* OK [PERMANENTFLAGS ()] No permanent flags permitted\r\n";

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
        tracing::trace!("Received {raw_msg} in state {:?}", self.state);
        if let IMAPState::WaitingForAuth(tag) = &self.state.clone() {
            return match crate::utils::DECODER.decode(raw_msg) {
                Err(_) => Ok(vec![format!("{} BAD INVALID BASE64", tag)
                    .as_bytes()
                    .to_vec()]),
                Result::Ok(decoded) => {
                    let (usrname, password) = utils::seperate_login(decoded)?;
                    dbg!(&usrname, &password);
                    let result = self.db.lock().await.check_user(&usrname, &password).await;

                    if let Some(a) = result {
                        self.state = IMAPState::Authed(a);
                        Ok(vec![format!("{} OK Success\r\n", tag).as_bytes().to_vec()])
                    } else {
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
                let mut username = msg.next().context("should provide username")?;
                let mut password = msg.next().context("should provice password")?;
                //NOTE: python's imaplib submits passwords enclosed like this: \"password\"
                //so we will need to remove them
                //NOTE: this approach does't support passwords with spaces, but I think that's ok
                //for now
                password = &password[1..password.len() - 1];
                username = &username[1..username.len() - 1];
                dbg!(&username, &password);
                if let Some(x) = self.db.lock().await.check_user(username, password).await {
                    let good_msg = format!("{} OK LOGIN COMPLETED\r\n", tag);
                    self.state = IMAPState::Authed(x);
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
                                let (usrname, password) = utils::seperate_login(decoded)?;

                                let result =
                                    self.db.lock().await.check_user(&usrname, &password).await;

                                if let Some(a) = result {
                                    self.state = IMAPState::Authed(a);
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
            ("enable", IMAPState::Authed(_)) => {
                let response = format!("{} BAD NO EXTENSIONS SUPPORTED\r\n", tag);
                Ok(vec![response.as_bytes().to_vec()])
            }
            (x, IMAPState::Authed(id)) if x == "select" || x == "examine" => {
                let mailbox = match msg.next().context("should provide mailbox name") {
                    Err(_) => {
                        return Ok(vec![format!("{} BAD missing arguments\r\n", tag)
                            .as_bytes()
                            .to_vec()])
                    }
                    Result::Ok(a) => a.chars().filter(|c| c != &'"').collect::<String>(),
                };
                let db = self.db.lock().await;

                let m_id = match db.get_mailbox_id(id, &mailbox).await {
                    Err(x) => {
                        return Ok(vec![format!("{} BAD {}\r\n", tag, x).as_bytes().to_vec()])
                    }
                    Result::Ok(a) => a,
                };

                let unix_time = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .context("Time shouldn't go backwards")?;
                let seconds: u32 = unix_time.as_secs().try_into()?;

                let uid_validity = format!("* OK [UIDVALIDITY {}]\r\n", seconds)
                    .as_bytes()
                    .to_vec();

                let count = db
                    .mail_count(Some(m_id))
                    .await
                    .context("mail_count failed")?;
                let count_string = format!("* {} EXISTS\r\n", count).as_bytes().to_vec();

                let expected_uid = db.biggest_uid().await.unwrap_or(0) + 1;
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
                let mailbox_list = format!("* LIST () \"/\" {}\r\n", mailbox)
                    .as_bytes()
                    .to_vec();
                let response = vec![
                    count_string,
                    uid_validity,
                    expected_uid_string,
                    Self::FLAGS.to_vec(),
                    mailbox_list,
                    permanent_flags.to_vec(),
                    final_tagged,
                ];
                if x == "select" {
                    self.state = IMAPState::Selected(SelectedState {
                        read_only: false,
                        user_id: id,
                        mailbox_id: m_id,
                    });
                } else {
                    self.state = IMAPState::Selected(SelectedState {
                        read_only: true,
                        user_id: id,
                        mailbox_id: m_id,
                    })
                }
                Ok(response)
            }
            ("create", IMAPState::Authed(id)) => {
                let Some(mailbox_name) = msg.next() else {
                    return Ok(vec![format!("{} BAD didn't provide a name\r\n", tag)
                        .as_bytes()
                        .to_vec()]);
                };
                self.db
                    .lock()
                    .await
                    .create_mailbox(id, mailbox_name)
                    .await?;

                Ok(vec![format!("{} OK CREATE completed\r\n", tag)
                    .as_bytes()
                    .to_vec()])
            }
            ("delete", IMAPState::Authed(id)) => {
                let Some(mailbox_name) = msg.next() else {
                    return Ok(vec![format!("{} BAD didn't provide a name\r\n", tag)
                        .as_bytes()
                        .to_vec()]);
                };
                let db = self.db.lock().await;
                let mailbox_id = db.get_mailbox_id(id, mailbox_name).await?;
                db.delete_mailbox(mailbox_id).await?;
                Ok(vec![format!("{} OK DELETE completed\r\n", tag)
                    .as_bytes()
                    .to_vec()])
            }
            ("rename", IMAPState::Authed(id)) => {
                let Some(mailbox_name) = msg.next() else {
                    return Ok(vec![format!("{} BAD didn't provide a name\r\n", tag)
                        .as_bytes()
                        .to_vec()]);
                };
                let db = self.db.lock().await;
                let Result::Ok(mailbox_id) = db.get_mailbox_id(id, mailbox_name).await else {
                    return Ok(vec![format!("{} BAD no such mailbox\r\n", tag)
                        .as_bytes()
                        .to_vec()]);
                };
                db.rename_mailbox(mailbox_name, mailbox_id).await?;
                if mailbox_name == "INBOX" {
                    //as per the rfc
                    db.create_mailbox(id, "INBOX").await?;
                }
                Ok(vec![format!("{} OK RENAME completed\r\n", tag)
                    .as_bytes()
                    .to_vec()])
            }
            ("subscribe", IMAPState::Authed(id)) => {
                //TODO
                Ok(vec![format!(
                    "{} NO cannot subscribe to mailboxes\r\n",
                    tag
                )
                .as_bytes()
                .to_vec()])
            }
            ("unsubscribe", IMAPState::Authed(x)) => {
                //TODO
                Ok(vec![format!(
                    "{} NO cannot unsubscribe from mailboxes\r\n",
                    tag
                )
                .as_bytes()
                .to_vec()])
            }
            ("list", IMAPState::Authed(id)) => {
                let mut mailboxes = self
                    .db
                    .lock()
                    .await
                    .get_mailbox_names_for_user(id)
                    .await
                    .context(anyhow!("couldn't get mailbox names"))?;
                mailboxes = mailboxes
                    .iter()
                    .map(|v| format!("* LIST () \"/\" {}\r\n", v))
                    .collect();
                mailboxes.push(format!("{} OK LIST completed\r\n", tag));
                let mailboxes = mailboxes
                    .iter()
                    .map(|e| e.as_bytes().to_vec())
                    .collect::<Vec<Vec<u8>>>();

                Ok(mailboxes)
            }
            ("namespace", IMAPState::Authed(x)) => {
                //TODO
                Ok(vec![Self::NAMESPACE.to_vec()])
            }
            ("status", IMAPState::Authed(id)) => {
                let mailbox_name = msg.next().context("should provide mailbox name")?;

                //remove the parentheses (UIDNEXT MESSAGES) -> UIDNEXT MESSAGES
                let rest = msg
                    .map(|m| m.chars().filter(|c| c.is_alphabetic()).collect::<String>())
                    .collect::<Vec<_>>();
                let db = self.db.lock().await;
                let mailbox_id = db.get_mailbox_id(id, mailbox_name).await?;
                //hate this type
                let mut result: Vec<Vec<u8>> = vec![];

                dbg!(&rest);
                for attr in rest {
                    match attr.as_str() {
                        "MESSAGES" => {
                            let msg_count = db.mail_count(Some(mailbox_id)).await?;
                            result.push(format!("MESSAGES {}", msg_count).as_bytes().to_vec());
                        }
                        "UIDNEXT" => {
                            let nextuid = db.biggest_uid().await.unwrap_or(0) + 1;
                            result.push(format!("UIDNEXT {}", nextuid).as_bytes().to_vec());
                        }
                        "UNSEEN" => {
                            let count = db
                                .mail_count_with_flags(mailbox_id, vec![(IMAPFlags::Seen, false)])
                                .await?;
                            result.push(format!("UNSEEN {}", count).as_bytes().to_vec());
                        }
                        "DELETED" => {
                            let count = db
                                .mail_count_with_flags(mailbox_id, vec![(IMAPFlags::Deleted, true)])
                                .await?;
                            result.push(format!("DELETED {}", count).as_bytes().to_vec());
                        }
                        "SIZE" => {
                            //probably just do a sum() in sql, doesn't need to be accurate
                        }
                        _ => continue,
                    }
                }
                let response1_raw = String::from_utf8(result.join(" ".as_bytes()))?;
                let response1 = format!("* STATUS {} ({})\r\n", mailbox_name, response1_raw)
                    .as_bytes()
                    .to_vec();
                let response2 = format!("{} OK STATUS completed\r\n", tag)
                    .as_bytes()
                    .to_vec();

                Ok(vec![response1, response2])
            }
            ("append", IMAPState::Authed(x)) => {
                //TODO
                Err(anyhow!("not implemented"))
            }
            ("idle", IMAPState::Authed(x)) => {
                //TODO
                Err(anyhow!("not implemented"))
            }
            (x, IMAPState::Selected(y)) if x == "close" || x == "unselect" => {
                self.state = IMAPState::Authed(y.user_id);
                if x == "close" && !y.read_only {
                    //TODO: delete pending mail permanently
                }
                let response = if x == "close" {
                    format!("{} OK CLOSE completed\r\n", tag)
                        .as_bytes()
                        .to_vec()
                } else {
                    format!("{} OK UNSELECT completed\r\n", tag)
                        .as_bytes()
                        .to_vec()
                };
                Ok(vec![response])
            }
            (
                "expunge",
                IMAPState::Selected(SelectedState {
                    read_only: false,
                    user_id: _,
                    mailbox_id: x,
                }),
            ) => {
                //TODO tell the uids of deleted messages like the rfc
                self.db.lock().await.expunge(x).await?;
                Ok(vec![format!("{} OK EXPUNGE completed\r\n", tag)
                    .as_bytes()
                    .to_vec()])
            }
            ("search", IMAPState::Selected(_)) => {
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
