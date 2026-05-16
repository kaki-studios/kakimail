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
            "copy" => super::copy::copy_or_uid(tag, rest, state, db, true).await,
            "expunge" => super::expunge::expunge_or_uid(tag, rest, state, db, true).await,
            "fetch" => super::fetch::fetch_or_uid(tag, rest, state, db, true).await,
            "move" => super::move_op::move_or_uid(tag, rest, state, db, true).await,
            "search" => super::search::search_or_uid(tag, rest, state, db, true).await,
            "store" => super::store::store_or_uid(tag, rest, state, db, true).await,
            x => Err(anyhow!("uid command unknown: {:?}", x)),
        }
    }
}
