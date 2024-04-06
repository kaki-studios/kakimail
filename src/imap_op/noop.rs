use anyhow::{Context, Ok};

use crate::imap::IMAPOp;

pub struct Noop;

impl IMAPOp for Noop {
    async fn process(
        raw_msg: &str,
        //any state
        state: crate::imap::IMAPState,
        #[allow(unused)] db: std::sync::Arc<tokio::sync::Mutex<crate::database::DBClient>>,
    ) -> anyhow::Result<(Vec<Vec<u8>>, crate::imap::IMAPState, bool)> {
        let mut msg = raw_msg.split_whitespace();
        let tag = msg.next().context("no tag")?;
        let resp = vec![format!("{} OK NOOP completed\r\n", tag).as_bytes().to_vec()];
        Ok((resp, state, false))
    }
}
