use anyhow::*;
use tokio::net::*;

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
