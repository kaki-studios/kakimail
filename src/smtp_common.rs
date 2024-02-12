use anyhow::Context;
use anyhow::Result;
use base64::Engine;

//naïve
#[derive(Default, Clone, Debug, PartialEq, Eq)]
pub struct Mail {
    pub from: String,
    pub to: Vec<String>,
    pub data: String,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum State {
    Fresh,
    Greeted,
    Authed,
    ReceivingRcpt(Mail),
    ReceivingData(Mail),
    Received(Mail),
}

pub struct StateMachine {
    pub state: State,
    pub ehlo_greeting: String,
    pub require_auth: bool,
}

/// An state machine capable of handling SMTP commands
/// for receiving mail.
/// Use handle_smtp() to handle a single command.
/// The return value from handle_smtp() is the response
/// that should be sent back to the client.
/// Copied from edgemail, temporary
impl StateMachine {
    pub const OH_HAI: &'static [u8] = b"220 kakimail\n";
    pub const KK: &'static [u8] = b"250 Ok\n";
    pub const AUTH_OK: &'static [u8] = b"235 Ok\n";
    pub const AUTH_NOT_OK: &'static [u8] = b"535 Authentication error\n";
    pub const NOT_AUTHED_YET: &'static [u8] = b"530 Need authentication\n";
    pub const SEND_DATA_PLZ: &'static [u8] = b"354 End data with <CR><LF>.<CR><LF>\n";
    pub const KTHXBYE: &'static [u8] = b"221 Bye\n";
    pub const HOLD_YOUR_HORSES: &'static [u8] = &[];

    pub fn new(domain: impl AsRef<str>, require_auth: bool) -> Self {
        let domain = domain.as_ref();
        let ehlo_greeting = format!("250-{domain} Hello {domain}\n250 AUTH PLAIN LOGIN\n");
        Self {
            state: State::Fresh,
            ehlo_greeting,
            require_auth,
        }
    }

    /// Handles a single SMTP command and returns a proper SMTP response
    pub fn handle_smtp(&mut self, raw_msg: &str) -> Result<&[u8]> {
        tracing::trace!("Received {raw_msg} in state {:?}", self.state);
        let mut msg = raw_msg.split_whitespace();
        let command = msg.next().context("received empty command")?.to_lowercase();
        let state = std::mem::replace(&mut self.state, State::Fresh);
        match (command.as_str(), state) {
            ("ehlo", State::Fresh) => {
                tracing::trace!("Sending AUTH info");
                self.state = State::Greeted;
                Ok(self.ehlo_greeting.as_bytes())
            }
            ("helo", State::Fresh) => {
                self.state = State::Greeted;
                Ok(StateMachine::KK)
            }
            ("noop", _) | ("help", _) | ("info", _) | ("vrfy", _) | ("expn", _) => {
                tracing::trace!("Got {command}");
                Ok(StateMachine::KK)
            }
            ("rset", _) => {
                self.state = State::Fresh;
                Ok(StateMachine::KK)
            }
            ("auth", _) => {
                tracing::trace!("Acknowledging AUTH");
                let auth = msg.nth(1).context("should provide auth info")?;
                let engine = base64::engine::GeneralPurpose::new(
                    &base64::alphabet::STANDARD,
                    base64::engine::GeneralPurposeConfig::default(),
                );
                let decoded = engine.decode(auth).context("should be valid base64")?;
                let login = std::str::from_utf8(&decoded[0..])?;
                if login
                    == format!(
                        "\0{}\0{}",
                        std::env::var("USERNAME")?,
                        std::env::var("PASSWORD")?
                    )
                {
                    tracing::info!("success!, logged in!");
                    self.state = State::Authed;
                    return Ok(StateMachine::AUTH_OK);
                } else {
                    self.state = State::Greeted;
                }
                tracing::info!("wrong credentials: {login}");
                Ok(StateMachine::AUTH_NOT_OK)
            }
            ("mail", curr_state) => {
                if curr_state == State::Greeted && self.require_auth {
                    tracing::warn!("Didn't sign in!");
                    return Ok(StateMachine::NOT_AUTHED_YET);
                }
                tracing::trace!("Receiving MAIL");
                let from = msg.next().context("received empty MAIL")?;
                let from = from
                    .strip_prefix("FROM:")
                    .context("received incorrect MAIL")?;
                self.state = State::ReceivingRcpt(Mail {
                    from: from.to_string(),
                    ..Default::default()
                });
                Ok(StateMachine::KK)
            }
            ("rcpt", State::ReceivingRcpt(mut mail)) => {
                tracing::trace!("Receiving rcpt");
                let to = msg.next().context("received empty RCPT")?;
                let to = to.strip_prefix("TO:").context("received incorrect RCPT")?;
                let to = to.to_lowercase();
                if Self::legal_recipient(&to) {
                    mail.to.push(to);
                } else {
                    tracing::warn!("Illegal recipient: {to}")
                }
                self.state = State::ReceivingRcpt(mail);
                Ok(StateMachine::KK)
            }
            ("data", State::ReceivingRcpt(mail)) => {
                tracing::trace!("Receiving data");
                self.state = State::ReceivingData(mail);
                Ok(StateMachine::SEND_DATA_PLZ)
            }
            ("quit", State::ReceivingData(mail)) => {
                tracing::trace!(
                    "Received data: FROM: {} TO:{} DATA:{}",
                    mail.from,
                    mail.to.join(", "),
                    mail.data
                );
                self.state = State::Received(mail);
                Ok(StateMachine::KTHXBYE)
            }
            ("quit", _) => {
                tracing::warn!("Received quit before getting any data");
                Ok(StateMachine::KTHXBYE)
            }
            (_, State::ReceivingData(mut mail)) => {
                tracing::trace!("Receiving data");
                let resp = if raw_msg.ends_with("\r\n.\r\n") {
                    StateMachine::KK
                } else {
                    StateMachine::HOLD_YOUR_HORSES
                };
                mail.data += &raw_msg;
                self.state = State::ReceivingData(mail);
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
}
