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
        let (cmd, rest) = args.split_once(" ").context("should be parseable")?;
        match cmd.to_lowercase().as_str() {
            //TODO copy, move, fetch, store
            "expunge" => super::expunge::expunge_or_uid(tag, rest, state, db, true).await,
            "search" => super::search::search_or_uid(tag, rest, state, db, true).await,
            x => Err(anyhow!("uid command unknown: {:?}", x)),
        }
    }
}
