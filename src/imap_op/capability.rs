use anyhow::Ok;

use crate::imap::{IMAPOp, ResponseInfo};

pub struct Capability;

const CAPABILITY: &'static [u8] =
    b"* CAPABILITY IMAP4rev2 IMAP4rev1 STARTTLS AUTH=PLAIN UIDPLUS MOVE LITERAL+\r\n";
impl IMAPOp for Capability {
    async fn process(
        tag: &str,
        _args: &str,
        state: crate::imap::IMAPState,
        _db: std::sync::Arc<tokio::sync::Mutex<crate::database::DBClient>>,
    ) -> anyhow::Result<(Vec<Vec<u8>>, crate::imap::IMAPState, ResponseInfo)> {
        let value2 = format!("{} OK CAPABILITY completed\r\n", tag);
        Ok((
            vec![CAPABILITY.to_vec(), value2.as_bytes().to_vec()],
            state,
            ResponseInfo::Regular,
        ))
    }
}
