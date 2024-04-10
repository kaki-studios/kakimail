use std::{sync::Arc, u8};

use anyhow::{anyhow, Context, Ok, Result};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    sync::Mutex,
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
}

pub struct IMAP {
    state: IMAPState,
    stream: StreamType,
    db: Arc<Mutex<database::DBClient>>,
    tls_acceptor: tokio_rustls::TlsAcceptor,
}

impl IMAP {
    //eg: "* OK [CAPABILITY STARTTLS AUTH=SCRAM-SHA-256 LOGINDISABLED IMAP4rev2] IMAP4rev2 Service Ready"
    const GREETING: &'static [u8] = b"* OK IMAP4rev2 Service Ready\r\n";
    const _HOLD_YOUR_HORSES: &'static [u8] = &[];
    // const FLAGS: &'static [u8] = b"* FLAGS (\\Answered \\Flagged \\Deleted \\Seen \\Draft)\r\n";
    // const CAPABILITY: &'static [u8] = b"* CAPABILITY IMAP4rev2 STARTTLS IMAP4rev1 AUTH=PLAIN\r\n";
    // const PERMANENT_FLAGS: &'static [u8] = b"* OK [PERMANENTFLAGS (\\Deleted \\Seen \\*)]\r\n";
    ///shouldn't exist in the future
    // const NAMESPACE: &'static [u8] = b"* NAMESPACE ((\"\" \"/\")) NIL NIL\r\n";
    // const NO_PERMANENT_FLAGS: &'static [u8] =
    // b"* OK [PERMANENTFLAGS ()] No permanent flags permitted\r\n";
    //used for e.g. APPEND
    // const DATETIME_FMT: &'static str = "%d-%b-%y %H:%M:%S %z";

    /// Creates a new server from a connected stream
    pub async fn new(
        stream: tokio::net::TcpStream,
        acceptor: tokio_rustls::TlsAcceptor,
        implicit_tls: bool,
    ) -> Result<Self> {
        if !implicit_tls {
            Ok(Self {
                stream: StreamType::Plain(stream),
                state: IMAPState::NotAuthed,
                db: Arc::new(Mutex::new(database::DBClient::new().await?)),
                tls_acceptor: acceptor,
            })
        } else {
            let tls_stream = acceptor.accept(stream).await?;

            Ok(Self {
                stream: StreamType::Tls(tls_stream),
                state: IMAPState::NotAuthed,
                db: Arc::new(Mutex::new(database::DBClient::new().await?)),
                tls_acceptor: acceptor,
            })
        }
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
    ) -> Result<(IMAPState, StreamType)> {
        if raw_msg == "\r\n" {
            return Ok((state, stream));
        }
        tracing::info!("Received {raw_msg} in state {:?}", state);
        let mut msg = raw_msg.split_whitespace();
        let tag = msg.next().context("received empty tag")?;
        let command = msg.next().context("received empty command")?.to_lowercase();

        dbg!(&command);
        let new = msg.clone();
        let args = new.collect::<Vec<&str>>().join(" ");
        //TODO
        //-implement the uid command
        //2. add extra `uid.rs` file like the other commands, and
        //make a base function for commands that can have uid
        let (resp, new_state, info) =
            exec_command(&command, tag, &args, state, db.clone()).await??;
        for item in &resp {
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
                dbg!(&args);
                //test
                let mut new = args.strip_suffix("\r\n").unwrap_or(&args).to_owned();
                new.push_str(&format!(" {}", extra));
                //TODO not working
                dbg!(&new);

                let (resp, new_state, info) =
                    exec_command(&command, tag, &new, state, db).await??;
                for item in resp {
                    stream.write_all(&item).await?;
                }
                if info == ResponseInfo::RedoForNextMsg {
                    return Err(anyhow!("2 times RedoForNextMsg!"));
                }
                state = new_state;
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
                )
                .await
                .ok();
                break;
            }
            let msg = std::str::from_utf8(&buf[0..n])?;
            dbg!(&msg);
            let (state, stream) = Self::handle_imap(
                self.stream,
                self.db.clone(),
                self.state,
                self.tls_acceptor.clone(),
                msg,
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
) -> Result<Result<(Response, IMAPState, ResponseInfo)>> {
    match command.to_lowercase().as_str() {
        "append" => Ok(imap_op::append::Append::process(tag, args, state, db).await),
        "capability" => Ok(imap_op::capability::Capability::process(tag, args, state, db).await),
        "create" => Ok(imap_op::create::Create::process(tag, args, state, db).await),
        "enable" => Ok(imap_op::enable::Enable::process(tag, args, state, db).await),
        "expunge" => Ok(imap_op::expunge::Expunge::process(tag, args, state, db).await),
        "login" => Ok(imap_op::login::Login::process(tag, args, state, db).await),
        "noop" => Ok(imap_op::noop::Noop::process(tag, args, state, db).await),
        "select" => Ok(imap_op::select::Select::process(tag, args, state, db).await),
        "status" => Ok(imap_op::status::Status::process(tag, args, state, db).await),
        "unselect" => Ok(imap_op::unselect::Unselect::process(tag, args, state, db).await),
        "authenticate" => {
            Ok(imap_op::authenticate::Authenticate::process(tag, args, state, db).await)
        }
        "close" => Ok(imap_op::close::Close::process(tag, args, state, db).await),
        "delete" => Ok(imap_op::delete::Delete::process(tag, args, state, db).await),
        "examine" => Ok(imap_op::examine::Examine::process(tag, args, state, db).await),
        "list" => Ok(imap_op::list::List::process(tag, args, state, db).await),
        "logout" => Ok(imap_op::logout::Logout::process(tag, args, state, db).await),
        "namespace" => Ok(imap_op::namespace::Namespace::process(tag, args, state, db).await),
        "rename" => Ok(imap_op::rename::Rename::process(tag, args, state, db).await),
        "starttls" => Ok(imap_op::starttls::StartTls::process(tag, args, state, db).await),
        "subscribe" => Ok(imap_op::subscribe::Subscribe::process(tag, args, state, db).await),
        "unsubscribe" => Ok(imap_op::unsubscribe::Unsubscribe::process(tag, args, state, db).await),
        _ => Err(anyhow!("invalid command")),
    }
}
