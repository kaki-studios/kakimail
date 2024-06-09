use anyhow::anyhow;

use crate::imap::{IMAPOp, IMAPState};

pub struct Fetch;

impl IMAPOp for Fetch {
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
        let IMAPState::Selected(_user) = state else {
            return Err(anyhow!("wrong state"));
        };

        Err(anyhow!("not implemented"))
    }
}
