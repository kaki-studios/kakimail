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
        let id = match state {
            IMAPState::Authed(id) => id,
            IMAPState::Selected(selected) => selected.user_id,
            _ => return Err(anyhow!("bad state")),
        };
        let parsed = crate::parsing::imap::parse_list(args)
            .map_err(|e| anyhow!("invalid RENAME args: {:?}", e))?;
        let Some(mailbox_name) = parsed.first() else {
            let resp = format!("{} BAD didn't provide a source name\r\n", tag)
                .as_bytes()
                .to_vec();
            return Ok((vec![resp], state, ResponseInfo::Regular));
        };
        let Some(new_name) = parsed.get(1) else {
            let resp = format!("{} BAD didn't provide a destination name\r\n", tag)
                .as_bytes()
                .to_vec();
            return Ok((vec![resp], state, ResponseInfo::Regular));
        };
        let db = db.lock().await;
        let Result::Ok(mailbox_id) = db.get_mailbox_id(id, mailbox_name).await else {
            let resp = format!("{} NO RENAME failed: no such mailbox\r\n", tag)
                .as_bytes()
                .to_vec();

            return Ok((vec![resp], state, ResponseInfo::Regular));
        };
        db.rename_mailbox(new_name, mailbox_id).await?;
        if mailbox_name.eq_ignore_ascii_case("INBOX") {
            //as per the rfc, renaming INBOX creates a new empty INBOX
            db.create_mailbox(id, "INBOX").await.ok();
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
