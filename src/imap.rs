use std::sync::Arc;

use tokio::sync::Mutex;

use crate::database;

pub struct IMAP {
    pub stream: tokio::net::TcpStream,
    pub db: Arc<Mutex<database::Client>>,
}

impl IMAP {
    pub async fn new() {}
}
