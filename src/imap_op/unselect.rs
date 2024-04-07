use crate::imap::IMAPOp;

use super::close::close_or_unselect;

pub struct Unselect;

impl IMAPOp for Unselect {
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
        close_or_unselect(tag, args, state, db, false).await
    }
}
