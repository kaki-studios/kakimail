use anyhow::Ok;

use crate::imap::{IMAPOp, ResponseInfo};

pub struct Noop;

impl IMAPOp for Noop {
    async fn process(
        tag: &str,
        _args: &str,
        state: crate::imap::IMAPState,
        _db: std::sync::Arc<tokio::sync::Mutex<crate::database::DBClient>>,
    ) -> anyhow::Result<(Vec<Vec<u8>>, crate::imap::IMAPState, ResponseInfo)> {
        let resp = vec![format!("{} OK NOOP completed\r\n", tag).as_bytes().to_vec()];
        Ok((resp, state, ResponseInfo::Regular))
    }
}
