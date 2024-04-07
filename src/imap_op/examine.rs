use crate::imap::IMAPOp;

pub struct Examine;

impl IMAPOp for Examine {
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
        crate::imap_op::select::select_or_examine(tag, args, state, db, false).await
    }
}
