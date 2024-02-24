use std::sync::Arc;

use anyhow::{Context, Result};
use tokio::sync::Mutex;

use crate::database;

#[derive(Clone, Debug, PartialEq, Eq)]
enum IMAPState {
    NotAuthed,
    Authed,
    Selected,
    Logout,
}
struct IMAPStateMachine {
    //add new fields as needed. prolly need TLS stuff later on
    state: IMAPState,
}

impl IMAPStateMachine {
    //eg: "* OK [CAPABILITY STARTTLS AUTH=SCRAM-SHA-256 LOGINDISABLED IMAP4rev2] IMAP4rev2 Service Ready"
    const GREETING: &'static [u8] = b"* OK IMAP4rev2 Service Ready";
    const KK: &'static [u8] = b"OK";

    fn new() -> Self {
        Self {
            state: IMAPState::NotAuthed,
        }
    }
    fn handle_imap(&mut self, raw_msg: &str) -> Result<&[u8]> {
        tracing::trace!("Received {raw_msg} in state {:?}", self.state);
        let mut msg = raw_msg.split_whitespace();
        let command = msg.next().context("received empty command")?.to_lowercase();
        let state = std::mem::replace(&mut self.state, IMAPState::NotAuthed);
        match (command.as_str(), state) {
            ("noop", _) => Ok(Self::KK),
            _ => anyhow::bail!(
                "Unexpected message received in state {:?}: {raw_msg}",
                self.state
            ),
        }
    }
}

pub struct IMAP {
    pub stream: tokio::net::TcpStream,
    pub state_machine: IMAPStateMachine,
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
        Ok(())
    }
}
