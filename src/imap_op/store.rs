use std::str::FromStr;

use anyhow::{anyhow, Context};

use crate::{
    database::{self, IMAPFlags, StoreMode},
    imap::{IMAPOp, IMAPState, ResponseInfo, SelectedState},
    imap_op::search::SequenceSet,
    parsing,
};

pub struct Store;

impl IMAPOp for Store {
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
        store_or_uid(tag, args, state, db, false).await
    }
}

pub(crate) async fn store_or_uid(
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
        mailbox_id,
        ..
    }) = state
    else {
        return Ok((
            vec![format!(
                "{} NO STORE failed: mailbox is read-only or not selected\r\n",
                tag
            )
            .into_bytes()],
            state,
            ResponseInfo::Regular,
        ));
    };

    let mut parts = args.trim_end_matches("\r\n").splitn(3, char::is_whitespace);
    let sequence_raw = parts.next().context("missing sequence set")?;
    let item_raw = parts.next().context("missing STORE data item")?;
    let flags_raw = parts.next().context("missing STORE flag list")?;

    let sequence_set = SequenceSet::from_str(sequence_raw)?;
    let (mode, silent) = parse_store_item(item_raw)?;
    let flags = parse_flag_list(flags_raw)?;
    let updates = db
        .lock()
        .await
        .store_flags(mailbox_id, sequence_set, uid, mode, &flags)?;

    let mut response = Vec::new();
    if !silent {
        for update in updates {
            let mut line = format!(
                "* {} FETCH (FLAGS ({})",
                update.seqnum,
                database::db_flag_to_readable_flag(&update.flags)
            );
            if uid {
                line.push_str(&format!(" UID {}", update.uid));
            }
            line.push_str(")\r\n");
            response.push(line.into_bytes());
        }
    }
    response.push(format!("{} OK STORE completed\r\n", tag).into_bytes());
    Ok((response, state, ResponseInfo::Regular))
}

fn parse_store_item(item: &str) -> anyhow::Result<(StoreMode, bool)> {
    let upper = item.to_ascii_uppercase();
    let silent = upper.ends_with(".SILENT");
    let base = upper.strip_suffix(".SILENT").unwrap_or(&upper);
    let mode = match base {
        "+FLAGS" => StoreMode::Add,
        "-FLAGS" => StoreMode::Remove,
        "FLAGS" => StoreMode::Replace,
        _ => return Err(anyhow!("invalid STORE data item: {}", item)),
    };
    Ok((mode, silent))
}

fn parse_flag_list(input: &str) -> anyhow::Result<Vec<IMAPFlags>> {
    let trimmed = input.trim();
    let inner = trimmed
        .strip_prefix('(')
        .and_then(|s| s.strip_suffix(')'))
        .unwrap_or(trimmed);
    parsing::imap::parse_list(inner)
        .map_err(|e| anyhow!("invalid flag list: {:?}", e))?
        .into_iter()
        .map(|flag| IMAPFlags::from_str(&flag))
        .collect()
}
