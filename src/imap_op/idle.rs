use anyhow::{anyhow, Ok};

use crate::imap::{IMAPOp, IMAPState, ResponseInfo};

pub struct Idle;

impl IMAPOp for Idle {
    async fn process(
        tag: &str,
        args: &str,
        state: crate::imap::IMAPState,
        _db: std::sync::Arc<tokio::sync::Mutex<crate::database::DBClient>>,
    ) -> anyhow::Result<(
        crate::imap::Response,
        crate::imap::IMAPState,
        crate::imap::ResponseInfo,
    )> {
        //TODO send update messages while waiting...
        //and test this command
        let IMAPState::Authed(_id) = state else {
            return Err(anyhow!("bad state"));
        };
        dbg!(&args);
        if args == "" {
            Ok((vec![], state, ResponseInfo::RedoForNextMsg))
        } else {
            Ok((
                vec![format!("{} OK IDLE terminated", tag).as_bytes().to_vec()],
                state,
                ResponseInfo::Regular,
            ))
        }
    }
}
