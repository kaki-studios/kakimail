use crate::imap::IMAPOp;

use super::subscribe::subscribe_or_unsubsribe;

pub struct Unsubscribe;

impl IMAPOp for Unsubscribe {
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
        subscribe_or_unsubsribe(tag, args, state, db, false).await
    }
}
