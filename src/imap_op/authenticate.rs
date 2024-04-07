use anyhow::{anyhow, Context, Ok};
use base64::Engine;

use crate::{
    imap::{IMAPOp, IMAPState, ResponseInfo},
    utils,
};

struct Authenticate;

impl IMAPOp for Authenticate {
    async fn process(
        tag: &str,
        args: &str,
        mut state: crate::imap::IMAPState,
        db: std::sync::Arc<tokio::sync::Mutex<crate::database::DBClient>>,
    ) -> anyhow::Result<(
        Vec<Vec<u8>>,
        crate::imap::IMAPState,
        crate::imap::ResponseInfo,
    )> {
        if state != IMAPState::NotAuthed {
            return Err(anyhow!("bad state"));
        }
        let mut msg = args.split_whitespace();
        let method = msg
            .next()
            .context("should provide auth mechanism")?
            .to_lowercase();
        //TODO support more methods
        if method != "plain" {
            Ok((
                vec![format!("{} BAD Unsupported Authentication Mechanism", tag)
                    .as_bytes()
                    .to_vec()],
                state,
                ResponseInfo::Regular,
            ))
        } else {
            let encoded = match msg.next() {
                None => {
                    //login will be in next message
                    let resp = b"+\r\n".to_vec();
                    return Ok((vec![resp], state, ResponseInfo::RedoForNextMsg));
                }
                Some(encoded) => encoded,
            };
            let resp = match crate::utils::DECODER.decode(encoded) {
                Err(_) => vec![format!("{} BAD INVALID BASE64\r\n", tag)
                    .as_bytes()
                    .to_vec()],
                Result::Ok(decoded) => {
                    let (usrname, password) = utils::seperate_login(decoded)?;

                    let result = db.lock().await.check_user(&usrname, &password).await;

                    if let Some(a) = result {
                        state = IMAPState::Authed(a);
                        vec![format!("{} OK Success\r\n", tag).as_bytes().to_vec()]
                    } else {
                        vec![format!("{} BAD Invalid Credentials\r\n", tag)
                            .as_bytes()
                            .to_vec()]
                    }
                }
            };
            Ok((resp, state, ResponseInfo::Regular))
        }
    }
}
