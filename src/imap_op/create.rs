use anyhow::{anyhow, Ok};

use crate::imap::{IMAPOp, IMAPState, ResponseInfo};

pub struct Create;

impl IMAPOp for Create {
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
        let mut msg = args.split_whitespace();
        let IMAPState::Authed(id) = state else {
            return Err(anyhow!("bad state"));
        };
        let Some(mailbox_name) = msg.next() else {
            let resp = format!("{} BAD didn't provide a name\r\n", tag)
                .as_bytes()
                .to_vec();

            return Ok((vec![resp], state, ResponseInfo::Regular));
        };
        db.lock().await.create_mailbox(id, mailbox_name).await?;

        Ok((
            vec![format!("{} OK CREATE completed\r\n", tag)
                .as_bytes()
                .to_vec()],
            state,
            ResponseInfo::Regular,
        ))
    }
}
