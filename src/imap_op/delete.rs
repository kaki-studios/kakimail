use anyhow::anyhow;

use crate::imap::{IMAPOp, IMAPState, ResponseInfo};

pub struct Delete;

impl IMAPOp for Delete {
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
        let IMAPState::Authed(id) = state else {
            return Err(anyhow!("bad state"));
        };
        let mut msg = args.split_whitespace();
        let Some(mailbox_name) = msg.next() else {
            let resp = format!("{} BAD didn't provide a name\r\n", tag)
                .as_bytes()
                .to_vec();
            return Ok((vec![resp], state, ResponseInfo::Regular));
        };
        let db = db.lock().await;
        let mailbox_id = db.get_mailbox_id(id, mailbox_name).await?;
        db.delete_mailbox(mailbox_id).await?;
        Ok((
            vec![format!("{} OK DELETE completed\r\n", tag)
                .as_bytes()
                .to_vec()],
            state,
            ResponseInfo::Regular,
        ))
    }
}
