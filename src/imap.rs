use std::{io::BufRead, sync::Arc, u8, vec};

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
    const CAPABILITY: &'static [u8] = b"* CAPABILITY IMAP4rev2 IMAP4rev1 AUTH=PLAIN\r\n";
    const PERMANENT_FLAGS: &'static [u8] = b"* OK [PERMANENTFLAGS (\\Deleted \\Seen \\*)]\r\n";
    ///shouldn't exist in the future
    const NAMESPACE: &'static [u8] = b"* NAMESPACE ((\"\" \"/\")) NIL NIL\r\n";
    const NO_PERMANENT_FLAGS: &'static [u8] =
        b"* OK [PERMANENTFLAGS ()] No permanent flags permitted\r\n";
    //used for e.g. APPEND
    const DATETIME_FMT: &'static str = "%d-%b-%y %H:%M:%S %z";

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
        let mut msg = raw_msg.split_whitespace();
        let tag = msg.next().context("received empty tag")?;
        let mut command = msg.next().context("received empty command")?.to_lowercase();
        let mut uid = false;
        if command.as_str() == "uid" {
            uid = true;
            command = msg
                .next()
                .context("uid command should provide actual command")?
                .to_lowercase();
        }

        let state = self.state.clone();
        match (command.as_str(), state) {
            //ANY STATE
            ("noop", _) => {
                let value = format!("{} OK NOOP completed\r\n", tag);
                Ok(vec![value.as_bytes().to_vec()])
            }
            ("capability", _) => {
                let value2 = format!("{} OK CAPABILITY completed\r\n", tag);
                Ok(vec![Self::CAPABILITY.to_vec(), value2.as_bytes().to_vec()])
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
                    //kinda sketchy, can overflow and also allocates 1kb of memory!
                    let mut buf = [0; 1024];
                    let encoded = match msg.next() {
                        None => {
                            //login will be in next message
                            self.stream.write_all("+\r\n".as_bytes()).await?;
                            let n = self.stream.read(&mut buf).await?;
                            std::str::from_utf8(&buf[..n])?
                        }
                        Some(encoded) => encoded,
                    };
                    dbg!(&encoded);

                    match crate::utils::DECODER.decode(encoded) {
                        Err(_) => Ok(vec![format!("{} BAD INVALID BASE64\r\n", tag)
                            .as_bytes()
                            .to_vec()]),
                        Result::Ok(decoded) => {
                            let (usrname, password) = utils::seperate_login(decoded)?;

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

                let expected_uid = db.biggest_uid().await.unwrap_or(-1) + 1;
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
                let mailbox_name = msg.next().context("should provide mailbox name")?;
                let mailbox_id = self
                    .db
                    .lock()
                    .await
                    .get_mailbox_id(id, mailbox_name)
                    .await?;
                self.db
                    .lock()
                    .await
                    .change_mailbox_subscribed(mailbox_id, true)
                    .await?;
                Ok(vec![format!("{} OK SUBSCRIBE completed\r\n", tag)
                    .as_bytes()
                    .to_vec()])
            }
            ("unsubscribe", IMAPState::Authed(id)) => {
                let mailbox_name = msg.next().context("should provide mailbox name")?;
                let mailbox_id = self
                    .db
                    .lock()
                    .await
                    .get_mailbox_id(id, mailbox_name)
                    .await?;
                self.db
                    .lock()
                    .await
                    .change_mailbox_subscribed(mailbox_id, false)
                    .await?;
                Ok(vec![format!("{} OK UNSUBSCRIBE completed\r\n", tag)
                    .as_bytes()
                    .to_vec()])
            }
            ("list", IMAPState::Authed(id)) => {
                //FIX this
                let mut mailboxes = self
                    .db
                    .lock()
                    .await
                    .get_mailbox_names_for_user(id)
                    .await
                    .context(anyhow!("couldn't get mailbox names"))?;
                dbg!(msg.collect::<Vec<&str>>());
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
            ("namespace", IMAPState::Authed(_id)) => {
                //idk
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
                            let nextuid = db.biggest_uid().await.unwrap_or(-1) + 1;
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
                            //TODO
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
            //FIX needs to support IMAPState::Selected(x) (might need a new match arm)
            ("append", IMAPState::Authed(id)) => {
                //FIX set flags and the parsed datetime in the final message
                let _mailbox_name = msg.next().context("should provide mailbox name")?;
                let mailbox_id = self
                    .db
                    .lock()
                    .await
                    .get_mailbox_id(id, _mailbox_name)
                    .await?;
                //dirty trick
                //(\Flag) "date"
                //into ["(\Flag", "\"date\""]
                let mut rest = msg
                    .collect::<Vec<&str>>()
                    .join(" ")
                    .split(|c| c == ')' || c == '"')
                    // //also a bit dirty
                    .map(|e| e.strip_prefix(" ").unwrap_or(e))
                    .map(|e| e.to_string())
                    .filter(|e| e != "")
                    .collect::<Vec<String>>();
                let msg_size = rest.pop().context("should provide message literal")?;
                let count = msg_size
                    .chars()
                    .filter(|c| c.is_digit(10))
                    .collect::<String>();
                let count = count.parse::<usize>()?;
                //as in {394+}
                let mail_data: String;
                if !msg_size.ends_with("+}") {
                    //yeah ik were doing stream stuff in the statemachine
                    self.stream
                        .write_all("+ Ready for literal data\r\n".as_bytes())
                        .await?;
                    let mut buf = vec![0_u8; count];
                    self.stream.read_exact(&mut buf).await?;
                    // dbg!(std::str::from_utf8(&buf)?);
                    mail_data = String::from_utf8(buf)?;
                } else {
                    mail_data = raw_msg[raw_msg.len() - count..].to_string();
                }
                let mut datetime = None;
                // dbg!(&rest);

                for arg in &rest {
                    // dbg!(&arg);
                    if arg.starts_with("(") {
                        // dbg!(arg);
                        let _stripped = arg
                            .strip_prefix("(")
                            .context("should begin with (")?
                            .strip_suffix(")")
                            .context("should end with )")?;
                        // dbg!(stripped);
                        //the flags SHOULD be set in the resulting message...
                        //TODO
                    } else {
                        if datetime.is_none() {
                            let stripped_arg =
                                arg.chars().filter(|c| c != &'"').collect::<String>();
                            datetime =
                                chrono::DateTime::parse_from_str(&stripped_arg, DATETIME_FMT)
                                    .map_err(|e| tracing::error!("{}", e))
                                    .ok();
                        }
                    }
                    // dbg!(arg);
                }

                // dbg!(&datetime);
                let mut recipients = vec![];
                let mut from = "".to_string();
                for line in mail_data.lines() {
                    // dbg!(&line);
                    match line.split_once(": ") {
                        Some(("From", x)) => {
                            let start_index = x.find("<").map(|e| e + 1);
                            let end_index = x.find(">");
                            let indices = start_index.zip(end_index).unwrap_or((0, 1));
                            from = x[indices.0..indices.1].to_string();
                        }
                        Some(("To", x)) => {
                            recipients.push(x.to_string());
                        }
                        _ => {}
                    }
                }
                let mail = crate::smtp_common::Mail {
                    from,
                    to: recipients,
                    data: mail_data,
                };
                // dbg!(&mail);
                //TODO:
                //-date (change db replicate() function)
                self.db.lock().await.replicate(mail, mailbox_id).await?;
                let unix_time = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .context("Time shouldn't go backwards")?;
                let seconds: u32 = unix_time.as_secs().try_into()?;
                let _resp = format!(
                    "{} OK [APPENDUID {} {}] APPEND completed\r\n",
                    tag,
                    seconds,
                    self.db.lock().await.biggest_uid().await.unwrap_or(0)
                );
                Ok(vec![_resp.as_bytes().to_vec()])
            }
            ("idle", IMAPState::Authed(x)) => {
                //TODO
                Err(anyhow!("not implemented"))
            }
            (x, IMAPState::Selected(y)) if x == "close" || x == "unselect" => {
                self.state = IMAPState::Authed(y.user_id);
                if x == "close" && !y.read_only {
                    //TODO: delete pending mail permanently
                    self.db.lock().await.expunge(y.mailbox_id, None).await?;
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
                let uid_range = if uid {
                    let mut test = msg
                        .next()
                        .context("should provide range")?
                        .split_once(":")
                        .map(|(a, b)| (a.parse::<i32>().ok(), b.parse::<i32>().ok()))
                        .context("should work")?;
                    if test.0 > test.1 {
                        std::mem::swap(&mut test.0, &mut test.1);
                    }
                    //cool
                    //turn a (Option<T>, Option<T>) to a Option<(T, T)>
                    test.0.zip(test.1)
                } else {
                    None
                };
                let results = self.db.lock().await.expunge(x, uid_range).await?;
                let mut strings = results
                    .iter()
                    .map(|i| format!("* {} EXPUNGE\r\n", i).as_bytes().to_vec())
                    .collect::<Vec<_>>();
                strings.push(
                    format!("{} OK EXPUNGE completed\r\n", tag)
                        .as_bytes()
                        .to_vec(),
                );

                Ok(strings)
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
            buf = [0; 65536];
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
