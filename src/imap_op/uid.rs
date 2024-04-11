use anyhow::{anyhow, Context};

use crate::imap::IMAPOp;

pub struct Uid;

impl IMAPOp for Uid {
    async fn process(
        tag: &str,
        args: &str,
        state: crate::imap::IMAPState,
        db: std::sync::Arc<tokio::sync::Mutex<crate::database::DBClient>>,
    ) -> anyhow::Result<(
        crate::imap::Response,
        crate::imap::IMAPState,
        crate::imap::ResponseInfo,
    )> {
        let mut msg = args.split_whitespace();
        match msg.next().context("")? {
            //TODO copy, move, fetch, store
            "expunge" => {
                let new_args = msg.collect::<Vec<&str>>().join(" ");
                super::expunge::expunge_or_uid(tag, &new_args, state, db, true).await
            }
            x => Err(anyhow!("uid command unknown: {:?}", x)),
        }
    }
}
