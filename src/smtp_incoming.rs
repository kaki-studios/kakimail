use std::sync::Arc;

use anyhow::*;
use tokio::{net::*, sync::Mutex};

use crate::database;

enum State {
    Fresh,
    Greeted,
    ReceivingRcpt(Mail),
    ReceivingData(Mail),
    Received(Mail),
}

pub struct Mail {
    pub from: String,
    pub to: Vec<String>,
    pub data: String,
}

struct StateMachine {
    state: State,
    ehlo_greeting: String,
}

pub struct SmtpIncoming {
    stream: tokio::net::TcpStream,
    state_machine: StateMachine,
    db: Arc<Mutex<database::Client>>,
}
