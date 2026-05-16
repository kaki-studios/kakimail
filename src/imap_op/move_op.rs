use anyhow::anyhow;

use crate::{
    imap::{IMAPOp, IMAPState, ResponseInfo},
    imap_op::search::SequenceSet,
};

pub struct Move;

impl IMAPOp for Move {
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
        move_or_uid(tag, args, state, db, false).await
    }
}

pub(crate) async fn move_or_uid(
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
    if selected.read_only {
        return Ok((
            vec![format!("{} NO MOVE failed: mailbox is read-only\r\n", tag).into_bytes()],
            state,
            ResponseInfo::Regular,
        ));
    }

    let (sequence_set, mailbox_name) = super::copy::parse_copy_like_args(args)?;
    let db = db.lock().await;
    let dest_mailbox_id = match db.get_mailbox_id(selected.user_id, &mailbox_name).await {
        Ok(id) => id,
        Err(e) => {
            return Ok((
                vec![format!("{} NO MOVE failed: {}\r\n", tag, e).into_bytes()],
                state,
                ResponseInfo::Regular,
            ))
        }
    };
    if dest_mailbox_id == selected.mailbox_id {
        return Ok((
            vec![format!(
                "{} NO MOVE failed: source and destination are the same\r\n",
                tag
            )
            .into_bytes()],
            state,
            ResponseInfo::Regular,
        ));
    }

    let copied = db
        .copy_messages(
            selected.mailbox_id,
            dest_mailbox_id,
            sequence_set.clone(),
            uid,
        )
        .await?;
    let expunged = db.delete_messages(selected.mailbox_id, sequence_set, uid)?;

    let mut response = expunged
        .iter()
        .map(|seqnum| format!("* {} EXPUNGE\r\n", seqnum).into_bytes())
        .collect::<Vec<_>>();
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
    response.push(format!("{} OK{} MOVE completed\r\n", tag, response_code).into_bytes());
    Ok((response, state, ResponseInfo::Regular))
}
