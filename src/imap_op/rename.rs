use anyhow::{anyhow, Ok};

use crate::imap::{IMAPOp, IMAPState, ResponseInfo};

pub struct Rename;

impl IMAPOp for Rename {
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
        let db = db.lock().await;
        let Result::Ok(mailbox_id) = db.get_mailbox_id(id, mailbox_name).await else {
            let resp = format!("{} BAD no such mailbox\r\n", tag)
                .as_bytes()
                .to_vec();

            return Ok((vec![resp], state, ResponseInfo::Regular));
        };
        db.rename_mailbox(mailbox_name, mailbox_id).await?;
        if mailbox_name == "INBOX" {
            //as per the rfc
            db.create_mailbox(id, "INBOX").await?;
        }
        Ok((
            vec![format!("{} OK RENAME completed\r\n", tag)
                .as_bytes()
                .to_vec()],
            state,
            ResponseInfo::Regular,
        ))
    }
}
