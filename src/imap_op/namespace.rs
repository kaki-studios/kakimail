use anyhow::anyhow;

use crate::imap::{IMAPOp, IMAPState, ResponseInfo};

pub struct Namespace;

///shouldn't exist in the future
const NAMESPACE: &'static [u8] = b"* NAMESPACE ((\"\" \"/\")) NIL NIL\r\n";

impl IMAPOp for Namespace {
    async fn process(
        tag: &str,
        _args: &str,
        state: crate::imap::IMAPState,
        _db: std::sync::Arc<tokio::sync::Mutex<crate::database::DBClient>>,
    ) -> anyhow::Result<(
        crate::imap::Response,
        crate::imap::IMAPState,
        crate::imap::ResponseInfo,
    )> {
        let IMAPState::Authed(_id) = state else {
            return Err(anyhow!("bad state"));
        };
        let resp = vec![
            NAMESPACE.to_vec(),
            format!("{} OK NAMESPACE completed\r\n", tag)
                .as_bytes()
                .to_vec(),
        ];
        //idk
        Ok((resp, state, ResponseInfo::Regular))
    }
}
