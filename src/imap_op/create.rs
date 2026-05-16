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
        let id = match state {
            IMAPState::Authed(id) => id,
            IMAPState::Selected(selected) => selected.user_id,
            _ => return Err(anyhow!("bad state")),
        };
        let parsed = crate::parsing::imap::parse_list(args)
            .map_err(|e| anyhow!("invalid CREATE args: {:?}", e))?;
        let Some(mailbox_name) = parsed.first() else {
            let resp = format!("{} BAD didn't provide a name\r\n", tag)
                .as_bytes()
                .to_vec();

            return Ok((vec![resp], state, ResponseInfo::Regular));
        };
        match db.lock().await.create_mailbox(id, mailbox_name).await {
            Result::Ok(_) => Ok((
                vec![format!("{} OK CREATE completed\r\n", tag)
                    .as_bytes()
                    .to_vec()],
                state,
                ResponseInfo::Regular,
            )),
            Err(e) => {
                let resp = format!("{} NO CREATE failed: {}\r\n", tag, e.to_string())
                    .as_bytes()
                    .to_vec();

                return Ok((vec![resp], state, ResponseInfo::Regular));
            }
        }
    }
}
