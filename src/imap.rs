use std::time::Duration;
use std::{sync::Arc, u8};

use anyhow::{anyhow, Context, Ok, Result};
use tokio::sync::mpsc::Receiver;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    sync::{mpsc::Sender, Mutex},
};

use crate::{
    database::{self},
    imap_op,
    tls::StreamType,
};

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Copy)]
pub enum IMAPState {
    NotAuthed,
    ///userid
    Authed(i32),
    Selected(SelectedState),
    Logout,
}

#[derive(PartialEq, PartialOrd, Eq, Debug, Clone, Copy)]
pub struct SelectedState {
    pub read_only: bool,
    pub user_id: i32,
    pub mailbox_id: i32,
}

pub type Response = Vec<Vec<u8>>;
pub trait IMAPOp {
    async fn process(
        tag: &str,
        args: &str,
        state: IMAPState,
        db: Arc<Mutex<database::DBClient>>,
    ) -> Result<(Response, IMAPState, ResponseInfo)>
    where
        Self: Sized;
}

#[derive(PartialEq, Eq)]
pub enum ResponseInfo {
    PromoteToTls,
    ///e.g. for authenticate or append where the args might come in the next msg
    RedoForNextMsg,
    Regular,
    Idle,
}

pub struct IMAP {
    state: IMAPState,
    stream: StreamType,
    db: Arc<Mutex<database::DBClient>>,
    tls_acceptor: tokio_rustls::TlsAcceptor,
    change_receiver: Arc<Mutex<Receiver<String>>>,
}

impl IMAP {
    //eg: "* OK [CAPABILITY STARTTLS AUTH=SCRAM-SHA-256 LOGINDISABLED IMAP4rev2] IMAP4rev2 Service Ready"
    const GREETING: &'static [u8] = b"* OK IMAP4rev2 Service Ready\r\n";
    const _HOLD_YOUR_HORSES: &'static [u8] = &[];

    /// Creates a new server from a connected stream
    pub async fn new(
        stream: tokio::net::TcpStream,
        acceptor: tokio_rustls::TlsAcceptor,
        implicit_tls: bool,
        tx: Sender<String>,
        rx: Arc<Mutex<Receiver<String>>>,
    ) -> Result<Self> {
        let stream_type = if !implicit_tls {
            StreamType::Plain(stream)
        } else {
            let tls_stream = acceptor.accept(stream).await?;
            StreamType::Tls(tls_stream)
        };
        Ok(Self {
            stream: stream_type,
            state: IMAPState::NotAuthed,
            db: Arc::new(Mutex::new(database::DBClient::new(tx).await?)),
            tls_acceptor: acceptor,
            change_receiver: rx,
        })
    }
    //not self bc we need ownership of the stream
    //but not ownership of self
    //and can't pass &mut self bc it would be
    //partially moved
    async fn handle_imap(
        mut stream: StreamType,
        db: Arc<Mutex<database::DBClient>>,
        mut state: IMAPState,
        tls_acceptor: tokio_rustls::TlsAcceptor,
        raw_msg: &str,
        changes: Arc<Mutex<Receiver<String>>>,
    ) -> Result<(IMAPState, StreamType)> {
        if raw_msg == "\r\n" {
            return Ok((state, stream));
        }
        tracing::info!("Received {raw_msg} \nIn state {:?}", state);
        let (tag, rest) = raw_msg.split_once(" ").context("did't receive tag")?;
        let (command, args) = rest
            .split_once(" ")
            .unwrap_or(rest.split_once("\r\n").context("didn't provide command")?);

        //TODO
        //-implement the uid command
        //2. add extra `uid.rs` file like the other commands, and
        //make a base function for commands that can have uid
        let (resp, new_state, info) = exec_command(command, tag, &args, state, db.clone()).await?;
        for item in &resp {
            if let Result::Ok(x) = String::from_utf8(item.to_vec()) {
                tracing::info!("writing: {}", x);
            } else {
                tracing::error!("somehow what we're writing isn't a valid string")
            }
            stream.write_all(&item).await?;
        }
        state = new_state;
        match info {
            ResponseInfo::Regular => {}
            ResponseInfo::PromoteToTls => {
                stream = stream.upgrade_to_tls(tls_acceptor).await?;
            }
            ResponseInfo::RedoForNextMsg => {
                let mut buf = [0; 1024];
                let n = stream.read(&mut buf).await?;

                let extra = std::str::from_utf8(&buf[..n])?;
                let mut new = args.strip_suffix("\r\n").unwrap_or(&args).to_owned();
                new.push_str(&format!(" {}", extra));
                dbg!(&new);

                let (resp, new_state, info) = exec_command(&command, tag, &new, state, db).await?;
                for item in resp {
                    stream.write_all(&item).await?;
                }
                if info == ResponseInfo::RedoForNextMsg {
                    return Err(anyhow!("2 times RedoForNextMsg!"));
                }
                state = new_state;
            }
            ResponseInfo::Idle => {
                let mut change_rx = changes.lock().await;
                let mut buf = [0u8; 1024];
                //idk why we have to import it, it complained otherwise
                use core::result::Result::Ok;
                tokio::select! {
                    result = tokio::time::timeout(Duration::from_secs(30 * 60), stream.read(&mut buf)) => {
                        match result {
                            Ok(Ok(bytes_read)) =>{
                                if bytes_read == 0 {
                                    return Err(anyhow!("read 0 bytes!"))
                                }
                                if &buf[..bytes_read] == b"DONE\r\n" {
                                    stream.write_all(format!("{} OK IDLE terminated\r\n", tag).as_bytes()).await?;
                                }
                            },
                            Ok(Err(err)) => {
                                tracing::error!("imap error: {}", err);
                            },
                            Err(_) => {
                                stream.write_all(b"* BYE IDLE timed out\r\n").await?;
                            }
                        }
                    }
                    change = change_rx.recv() => {
                        if let Some(change_str) = change {
                            tracing::info!("changes: {}" ,change_str);
                            stream.write_all(change_str.as_bytes()).await?;
                        }
                    }
                }
            }
        }

        Ok((state, stream))
    }
    pub async fn serve(mut self) -> Result<()> {
        self.greet().await?;
        let mut buf: [u8; 65536] = [0; 65536];
        loop {
            let n = self.stream.read(&mut buf).await?;

            if n == 0 {
                tracing::info!("Received EOF");
                Self::handle_imap(
                    self.stream,
                    self.db,
                    self.state,
                    self.tls_acceptor,
                    "logout",
                    self.change_receiver,
                )
                .await
                .ok();
                break;
            }
            let msg = std::str::from_utf8(&buf[0..n])?;
            let (state, stream) = Self::handle_imap(
                self.stream,
                self.db.clone(),
                self.state,
                self.tls_acceptor.clone(),
                msg,
                self.change_receiver.clone(),
            )
            .await
            .map_err(|e| {
                tracing::error!("{:?}", e);
                e
            })?;
            self.stream = stream;
            self.state = state;
            if self.state == IMAPState::Logout {
                break;
            }
            //clear
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

async fn exec_command(
    command: &str,

    tag: &str,
    args: &str,
    state: IMAPState,
    db: Arc<Mutex<database::DBClient>>,
) -> Result<(Response, IMAPState, ResponseInfo)> {
    match command.to_lowercase().as_str() {
        "append" => Ok(imap_op::append::Append::process(tag, args, state, db).await?),
        "capability" => Ok(imap_op::capability::Capability::process(tag, args, state, db).await?),
        "create" => Ok(imap_op::create::Create::process(tag, args, state, db).await?),
        "enable" => Ok(imap_op::enable::Enable::process(tag, args, state, db).await?),
        "expunge" => Ok(imap_op::expunge::Expunge::process(tag, args, state, db).await?),
        "login" => Ok(imap_op::login::Login::process(tag, args, state, db).await?),
        "noop" => Ok(imap_op::noop::Noop::process(tag, args, state, db).await?),
        "select" => Ok(imap_op::select::Select::process(tag, args, state, db).await?),
        "status" => Ok(imap_op::status::Status::process(tag, args, state, db).await?),
        "unselect" => Ok(imap_op::unselect::Unselect::process(tag, args, state, db).await?),
        "authenticate" => {
            Ok(imap_op::authenticate::Authenticate::process(tag, args, state, db).await?)
        }
        "close" => Ok(imap_op::close::Close::process(tag, args, state, db).await?),
        "delete" => Ok(imap_op::delete::Delete::process(tag, args, state, db).await?),
        "examine" => Ok(imap_op::examine::Examine::process(tag, args, state, db).await?),
        "list" => Ok(imap_op::list::List::process(tag, args, state, db).await?),
        "logout" => Ok(imap_op::logout::Logout::process(tag, args, state, db).await?),
        "namespace" => Ok(imap_op::namespace::Namespace::process(tag, args, state, db).await?),
        "rename" => Ok(imap_op::rename::Rename::process(tag, args, state, db).await?),
        "starttls" => Ok(imap_op::starttls::StartTls::process(tag, args, state, db).await?),
        "subscribe" => Ok(imap_op::subscribe::Subscribe::process(tag, args, state, db).await?),
        "unsubscribe" => {
            Ok(imap_op::unsubscribe::Unsubscribe::process(tag, args, state, db).await?)
        }
        "uid" => Ok(imap_op::uid::Uid::process(tag, args, state, db).await?),
        "idle" => Ok(imap_op::idle::Idle::process(tag, args, state, db).await?),
        "search" => Ok(imap_op::search::Search::process(tag, args, state, db).await?),
        _ => Err(anyhow!("invalid command")),
    }
}
