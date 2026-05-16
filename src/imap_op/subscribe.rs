use anyhow::{anyhow, Context};

use crate::imap::{IMAPOp, IMAPState, ResponseInfo};

pub struct Subscribe;

impl IMAPOp for Subscribe {
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
        subscribe_or_unsubsribe(tag, args, state, db, true).await
    }
}

pub(super) async fn subscribe_or_unsubsribe(
    tag: &str,
    args: &str,
    state: crate::imap::IMAPState,
    db: std::sync::Arc<tokio::sync::Mutex<crate::database::DBClient>>,
    subscribe: bool,
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
    let resp = if subscribe {
        format!("{} OK SUBSCRIBE completed\r\n", tag)
    } else {
        format!("{} OK UNSUBSCRIBE completed\r\n", tag)
    };
    let db = db.lock().await;

    let parsed = crate::parsing::imap::parse_list(args)
        .map_err(|e| anyhow!("invalid SUBSCRIBE args: {:?}", e))?;
    let mailbox_name = parsed.first().context("should provide mailbox name")?;
    let mailbox_id = db.get_mailbox_id(id, mailbox_name).await?;
    db.change_mailbox_subscribed(mailbox_id, subscribe).await?;
    Ok((vec![resp.as_bytes().to_vec()], state, ResponseInfo::Regular))
}
