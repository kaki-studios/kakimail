use anyhow::{anyhow, Context};

use crate::{
    database::IMAPFlags,
    imap::{IMAPOp, IMAPState, ResponseInfo},
    parsing,
};

pub struct Status;

impl IMAPOp for Status {
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
        let parsed =
            parsing::imap::parse_list(args).map_err(|e| anyhow!("invalid STATUS args: {:?}", e))?;
        let mailbox_name = parsed.first().context("should provide mailbox name")?;
        let rest = args
            .split_once('(')
            .and_then(|(_, r)| r.rsplit_once(')').map(|(attrs, _)| attrs))
            .unwrap_or("")
            .split_whitespace()
            .map(|attr| attr.to_ascii_uppercase())
            .collect::<Vec<_>>();
        let db = db.lock().await;
        let mailbox_id = db.get_mailbox_id(id, mailbox_name).await?;
        let mut result: Vec<String> = vec![];

        for attr in rest {
            match attr.as_str() {
                "MESSAGES" => {
                    let msg_count = db.mail_count(Some(mailbox_id)).await?;
                    result.push(format!("MESSAGES {}", msg_count));
                }
                "UIDNEXT" => {
                    let nextuid = db.next_uid().await;
                    result.push(format!("UIDNEXT {}", nextuid));
                }
                "UIDVALIDITY" => {
                    result.push(format!(
                        "UIDVALIDITY {}",
                        db.mailbox_uidvalidity(mailbox_id)
                    ));
                }
                "UNSEEN" => {
                    let count = db
                        .mail_count_with_flags(mailbox_id, vec![(IMAPFlags::Seen, false)])
                        .await?;
                    result.push(format!("UNSEEN {}", count));
                }
                "DELETED" => {
                    let count = db
                        .mail_count_with_flags(mailbox_id, vec![(IMAPFlags::Deleted, true)])
                        .await?;
                    result.push(format!("DELETED {}", count));
                }
                "SIZE" => result.push(format!("SIZE {}", db.status_size(mailbox_id).await?)),
                "RECENT" => result.push("RECENT 0".to_string()),
                _ => continue,
            }
        }
        let response1 = format!(
            "* STATUS {} ({})\r\n",
            parsing::imap::quote_string(mailbox_name),
            result.join(" ")
        )
        .into_bytes();
        let response2 = format!("{} OK STATUS completed\r\n", tag).into_bytes();

        Ok((vec![response1, response2], state, ResponseInfo::Regular))
    }
}
