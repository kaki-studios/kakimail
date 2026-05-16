use std::str::FromStr;

use anyhow::{anyhow, Context};

use crate::{
    imap::{IMAPOp, IMAPState, ResponseInfo},
    imap_op::search::SequenceSet,
    parsing,
};

pub struct Copy;

impl IMAPOp for Copy {
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
        copy_or_uid(tag, args, state, db, false).await
    }
}

pub(crate) async fn copy_or_uid(
    tag: &str,
    args: &str,
    state: crate::imap::IMAPState,
    db: std::sync::Arc<tokio::sync::Mutex<crate::database::DBClient>>,
    uid: bool,
) -> anyhow::Result<(
    crate::imap::Response,
    crate::imap::IMAPState,
    crate::imap::ResponseInfo,
)> {
    let IMAPState::Selected(selected) = state else {
        return Err(anyhow!("bad state"));
    };
    let (sequence_set, mailbox_name) = parse_copy_like_args(args)?;
    let db = db.lock().await;
    let dest_mailbox_id = match db.get_mailbox_id(selected.user_id, &mailbox_name).await {
        Ok(id) => id,
        Err(e) => {
            return Ok((
                vec![format!("{} NO COPY failed: {}\r\n", tag, e).into_bytes()],
                state,
                ResponseInfo::Regular,
            ))
        }
    };
    let copied = db
        .copy_messages(selected.mailbox_id, dest_mailbox_id, sequence_set, uid)
        .await?;
    let response_code = if copied.is_empty() {
        String::new()
    } else {
        let source = SequenceSet::from(
            copied
                .iter()
                .map(|item| item.source_uid)
                .collect::<Vec<_>>(),
        );
        let dest = SequenceSet::from(copied.iter().map(|item| item.dest_uid).collect::<Vec<_>>());
        format!(
            " [COPYUID {} {} {}]",
            db.mailbox_uidvalidity(dest_mailbox_id),
            source.to_string(),
            dest.to_string()
        )
    };
    Ok((
        vec![format!("{} OK{} COPY completed\r\n", tag, response_code).into_bytes()],
        state,
        ResponseInfo::Regular,
    ))
}

pub(crate) fn parse_copy_like_args(args: &str) -> anyhow::Result<(SequenceSet, String)> {
    let items =
        parsing::imap::parse_list(args).map_err(|e| anyhow!("invalid arguments: {:?}", e))?;
    let sequence = items.first().context("missing sequence set")?;
    let mailbox = items.get(1).context("missing destination mailbox")?;
    Ok((SequenceSet::from_str(sequence)?, mailbox.to_string()))
}
