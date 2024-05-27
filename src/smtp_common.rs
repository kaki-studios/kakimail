use std::sync::Arc;

use anyhow::Context;
use anyhow::Result;
use base64::Engine;
use tokio::sync::Mutex;

use crate::database;
use crate::utils;

//naïve
#[derive(Default, Clone, Debug, PartialEq, Eq)]
pub struct Mail {
    pub from: String,
    pub to: Vec<String>,
    pub data: String,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SMTPState {
    Fresh,
    Greeted,
    Authed(i32),
    ReceivingRcpt(Mail, Option<i32>),
    ReceivingData(Mail, Option<i32>),
    Received(Mail, Option<i32>),
}

pub struct SMTPStateMachine {
    pub state: SMTPState,
    pub ehlo_greeting: String,
    pub outgoing: bool,
}

/// An state machine capable of handling SMTP commands
/// for receiving mail.
/// Use handle_smtp() to handle a single command.
/// The return value from handle_smtp() is the response
/// that should be sent back to the client.
/// Copied from edgemail, temporary
impl SMTPStateMachine {
    pub const OH_HAI: &'static [u8] = b"220 smtp.kaki.foo ESMTP Server\r\n";
    pub const KK: &'static [u8] = b"250 Ok\r\n";
    pub const AUTH_OK: &'static [u8] = b"235 Ok\r\n";
    pub const AUTH_NOT_OK: &'static [u8] = b"535 Authentication error\r\n";
    pub const NOT_AUTHED_YET: &'static [u8] = b"530 Need authentication\r\n";
    pub const SEND_DATA_PLZ: &'static [u8] = b"354 End data with <CR><LF>.<CR><LF>\r\n";
    pub const READY_FOR_ENCRYPTION: &'static [u8] = b"220 Ready to start TLS\r\n";
    pub const KTHXBYE: &'static [u8] = b"221 Bye\r\n";
    pub const HOLD_YOUR_HORSES: &'static [u8] = &[];

    pub fn new(domain: impl AsRef<str>, outgoing: bool) -> Self {
        let domain = domain.as_ref();
        let ehlo_greeting =
            format!("250-{domain} Hello {domain}\r\n250-AUTH PLAIN LOGIN\r\n250 STARTTLS\r\n");
        Self {
            state: SMTPState::Fresh,
            ehlo_greeting,
            outgoing,
        }
    }

    /// Handles a single SMTP command and returns a proper SMTP response
    pub fn handle_smtp_incoming(&mut self, raw_msg: &str) -> Result<&[u8]> {
        tracing::info!("Received {raw_msg} in state {:?}", self.state);
        let mut msg = raw_msg.split_whitespace();
        let command = msg.next().context("received empty command")?.to_lowercase();
        let state = self.state.clone();
        match (command.as_str(), state) {
            ("ehlo", _) => {
                tracing::trace!("Sending AUTH info");
                self.state = SMTPState::Greeted;
                Ok(self.ehlo_greeting.as_bytes())
            }
            ("helo", SMTPState::Fresh) => {
                self.state = SMTPState::Greeted;
                Ok(SMTPStateMachine::KK)
            }
            ("starttls", _) => Ok(SMTPStateMachine::READY_FOR_ENCRYPTION),
            ("noop", _) | ("help", _) | ("info", _) | ("vrfy", _) | ("expn", _) => {
                tracing::trace!("Got {command}");
                Ok(SMTPStateMachine::KK)
            }
            ("rset", _) => {
                self.state = SMTPState::Fresh;
                Ok(SMTPStateMachine::KK)
            }
            ("mail", curr_state) => {
                tracing::trace!("Receiving MAIL");
                let from = msg.next().context("received empty MAIL")?;
                let from = from
                    .strip_prefix("FROM:")
                    .context("received incorrect MAIL")?;
                if self.outgoing {
                    if let SMTPState::Authed(x) = curr_state {
                        self.state = SMTPState::ReceivingRcpt(
                            Mail {
                                from: from.to_string(),

                                ..Default::default()
                            },
                            Some(x),
                        );
                    } else {
                        tracing::warn!("Didn't sign in!");
                        return Ok(SMTPStateMachine::NOT_AUTHED_YET);
                    }
                } else {
                    self.state = SMTPState::ReceivingRcpt(
                        Mail {
                            from: from.to_string(),

                            ..Default::default()
                        },
                        None,
                    );
                }
                Ok(SMTPStateMachine::KK)
            }
            ("rcpt", SMTPState::ReceivingRcpt(mut mail, x)) => {
                tracing::trace!("Receiving rcpt");
                let to = msg.next().context("received empty RCPT")?;
                let to = to.strip_prefix("TO:").context("received incorrect RCPT")?;
                let to = to.to_lowercase();
                if Self::legal_recipient(&to) {
                    mail.to.push(to);
                } else {
                    tracing::warn!("Illegal recipient: {to}")
                }
                self.state = SMTPState::ReceivingRcpt(mail, x);
                Ok(SMTPStateMachine::KK)
            }
            ("data", SMTPState::ReceivingRcpt(mail, x)) => {
                tracing::trace!("Receiving data");
                self.state = SMTPState::ReceivingData(mail, x);
                Ok(SMTPStateMachine::SEND_DATA_PLZ)
            }
            ("quit", SMTPState::ReceivingData(mail, x)) => {
                tracing::trace!(
                    "Received data: FROM: {} TO:{} DATA:{}",
                    mail.from,
                    mail.to.join(", "),
                    mail.data
                );
                self.state = SMTPState::Received(mail, x);
                Ok(SMTPStateMachine::KTHXBYE)
            }
            ("quit", _) => {
                tracing::warn!("Received quit before getting any data");
                Ok(SMTPStateMachine::KTHXBYE)
            }
            (_, SMTPState::ReceivingData(mut mail, x)) => {
                tracing::trace!("Receiving data");
                let resp = if raw_msg.ends_with("\r\n.\r\n") {
                    SMTPStateMachine::KK
                } else {
                    SMTPStateMachine::HOLD_YOUR_HORSES
                };
                mail.data += &raw_msg;
                self.state = SMTPState::ReceivingData(mail, x);
                Ok(resp)
            }
            _ => anyhow::bail!(
                "Unexpected message received in state {:?}: {raw_msg}",
                self.state
            ),
        }
    }

    /// Filter out admin, administrator, postmaster and hostmaster
    /// to prevent being able to register certificates for the domain.
    /// The check is over-eager, but it also makes it simpler.
    /// Assumes lowercased.
    fn legal_recipient(to: &str) -> bool {
        !to.contains("admin") && !to.contains("postmaster") && !to.contains("hostmaster")
    }

    pub async fn handle_smtp_outgoing(
        &mut self,
        raw_msg: &str,
        db: Arc<Mutex<database::DBClient>>,
    ) -> Result<&[u8]> {
        tracing::trace!("Received {raw_msg} in state {:?}", self.state);
        let mut msg = raw_msg.split_whitespace();
        let command = msg.next().context("received empty command")?.to_lowercase();
        dbg!(&command);
        match command.as_str() {
            "auth" => {
                let auth_type = msg
                    .next()
                    .context("should provide auth type")?
                    .to_lowercase();
                //TODO support other types
                if auth_type != "plain" {
                    tracing::warn!("used other auth mechanism: {}", auth_type);
                    self.state = SMTPState::Greeted;
                    return Ok(Self::AUTH_NOT_OK);
                }
                tracing::trace!("Acknowledging AUTH");
                let encoded = msg.next().context("should provide auth info").map_err(|e| {
                    tracing::error!("didn't have auth info");
                    e
                });
                match crate::utils::DECODER.decode(encoded?) {
                    Err(x) => {
                        self.state = SMTPState::Greeted;
                        tracing::error!("decode error: {}", x);
                        Ok(Self::AUTH_NOT_OK)
                    }
                    Result::Ok(decoded) => {
                        let (usrname, password) = utils::seperate_login(decoded)?;

                        let result = db.lock().await.check_user(&usrname, &password).await;

                        if let Some(_a) = result {
                            //TODO _a should be checked when sending to verify that the sender is actually the
                            //correct person, currently you can send emails on others behalf
                            //because of this

                            self.state = SMTPState::Authed(_a);
                            Ok(Self::AUTH_OK)
                        } else {
                            self.state = SMTPState::Greeted;
                            Ok(Self::AUTH_NOT_OK)
                        }
                    }
                }
            }
            _ => self.handle_smtp_incoming(raw_msg),
        }
    }
}
