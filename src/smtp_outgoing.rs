// use std::sync::Arc;

// use crate::utils::Mail;
use anyhow::Result;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    sync::Mutex,
};
// use tokio::sync::Mutex;

// use crate::smtp_incoming::StateMachine;

//hehe
#[allow(unused)]
pub struct SmtpOutgoing {
    stream: tokio::net::TcpStream,
    //tcp stream
    //also need something like this...
    // message_queue: Arc<Mutex<Vec<Mail>>>,
    // state_machine: StateMachine,
}

impl SmtpOutgoing {
    #[allow(unused)]
    pub async fn new(domain: impl AsRef<str>, stream: tokio::net::TcpStream) -> Result<Self> {
        Ok(Self {
            stream,
            // state_machine: StateMachine::new(domain),
            // message_queue: Arc::new(Mutex::new(Vec::new())),
        })
    }
    pub async fn serve(mut self) -> Result<()> {
        //TODO: greet here like in smtp_incoming.
        let mut buf = vec![0; 65536];
        loop {
            let n = self.stream.read(&mut buf).await?;
            if n == 0 {
                //quit
                tracing::info!("should quit on port 587");
                break;
            }
            let msg = std::str::from_utf8(&buf[0..n])?;
            tracing::info!("recieved traffic on port 587: {:?}", msg);
        }
        Ok(())
    }
}
