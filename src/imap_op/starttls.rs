use anyhow::Ok;

use crate::imap::{IMAPOp, ResponseInfo};

struct StartTls;

impl IMAPOp for StartTls {
    async fn process(
        tag: &str,
        _args: &str,
        state: crate::imap::IMAPState,
        _db: std::sync::Arc<tokio::sync::Mutex<crate::database::DBClient>>,
    ) -> anyhow::Result<(Vec<Vec<u8>>, crate::imap::IMAPState, ResponseInfo)> {
        let resp = format!("{} OK Begin TLS negotiation now\r\n", tag);
        Ok((
            vec![resp.as_bytes().to_vec()],
            state,
            ResponseInfo::PromoteToTls,
        ))
    }
}
