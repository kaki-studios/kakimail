use anyhow::anyhow;

use crate::imap::{IMAPOp, IMAPState, ResponseInfo};

pub struct Close;

impl IMAPOp for Close {
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
        close_or_unselect(tag, args, state, db, true).await
    }
}

pub(super) async fn close_or_unselect(
    tag: &str,
    _args: &str,
    mut state: crate::imap::IMAPState,
    db: std::sync::Arc<tokio::sync::Mutex<crate::database::DBClient>>,
    close: bool,
) -> anyhow::Result<(
    crate::imap::Response,
    crate::imap::IMAPState,
    crate::imap::ResponseInfo,
)> {
    let IMAPState::Selected(y) = state else {
        return Err(anyhow!("bad state"));
    };
    state = IMAPState::Authed(y.user_id);
    if close && !y.read_only {
        db.lock().await.expunge(y.mailbox_id, None).await?;
    }
    let response = if close {
        format!("{} OK CLOSE completed\r\n", tag)
            .as_bytes()
            .to_vec()
    } else {
        format!("{} OK UNSELECT completed\r\n", tag)
            .as_bytes()
            .to_vec()
    };
    Ok((vec![response], state, ResponseInfo::Regular))
}
