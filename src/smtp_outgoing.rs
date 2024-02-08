use std::sync::Arc;

use crate::utils::Mail;
use anyhow::Result;
use tokio::sync::Mutex;

use crate::smtp_incoming::StateMachine;

//hehe
#[allow(unused)]
struct SmtpOutgoing {
    stream: tokio::net::TcpStream,
    //tcp stream
    //also need something like this...
    message_queue: Arc<Mutex<Vec<Mail>>>,
    state_machine: StateMachine,
}

impl SmtpOutgoing {
    #[allow(unused)]
    pub async fn new(domain: impl AsRef<str>, stream: tokio::net::TcpStream) -> Result<Self> {
        Ok(Self {
            stream,
            state_machine: StateMachine::new(domain),
            message_queue: Arc::new(Mutex::new(Vec::new())),
        })
    }
}
