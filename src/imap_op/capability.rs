use anyhow::{Context, Ok};

use crate::imap::IMAPOp;

pub struct Capability;

const CAPABILITY: &'static [u8] = b"* CAPABILITY IMAP4rev2 STARTTLS IMAP4rev1 AUTH=PLAIN\r\n";
impl IMAPOp for Capability {
    async fn process(
        raw_msg: &str,
        state: crate::imap::IMAPState,
        _db: std::sync::Arc<tokio::sync::Mutex<crate::database::DBClient>>,
    ) -> anyhow::Result<(Vec<Vec<u8>>, crate::imap::IMAPState, bool)> {
        let tag = raw_msg
            .split_whitespace()
            .next()
            .context("should provide tag")?;

        let value2 = format!("{} OK CAPABILITY completed\r\n", tag);
        Ok((
            vec![CAPABILITY.to_vec(), value2.as_bytes().to_vec()],
            state,
            false,
        ))
    }
}
