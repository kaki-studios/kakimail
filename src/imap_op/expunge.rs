use anyhow::{anyhow, Context};

use crate::imap::{IMAPOp, IMAPState, ResponseInfo, SelectedState};

pub struct Expunge;

impl IMAPOp for Expunge {
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
        expunge_or_uid(tag, args, state, db, false).await
    }
}

pub(super) async fn expunge_or_uid(
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
    let IMAPState::Selected(SelectedState {
        read_only: false,
        user_id: _,
        mailbox_id: x,
    }) = state
    else {
        return Err(anyhow!("bad state"));
    };
    let mut msg = args.split_whitespace();

    let uid_range = if uid {
        let mut test = msg
            .next()
            .context("should provide range")?
            .split_once(":")
            .map(|(a, b)| (a.parse::<i32>().ok(), b.parse::<i32>().ok()))
            .context("should work")?;
        if test.0 > test.1 {
            std::mem::swap(&mut test.0, &mut test.1);
        }
        //cool
        //turn a (Option<T>, Option<T>) to a Option<(T, T)>
        test.0.zip(test.1)
    } else {
        None
    };
    let results = db.lock().await.expunge(x, uid_range).await?;
    let mut strings = results
        .iter()
        .map(|i| format!("* {} EXPUNGE\r\n", i).as_bytes().to_vec())
        .collect::<Vec<_>>();
    strings.push(
        format!("{} OK EXPUNGE completed\r\n", tag)
            .as_bytes()
            .to_vec(),
    );

    Ok((strings, state, ResponseInfo::Regular))
}
