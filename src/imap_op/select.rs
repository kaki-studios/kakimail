use anyhow::{anyhow, Context};
use nom::AsBytes;

use crate::imap::{IMAPOp, IMAPState, ResponseInfo, SelectedState};

pub struct Select;

const FLAGS: &'static [u8] = b"* FLAGS (\\Answered \\Flagged \\Deleted \\Seen \\Draft)\r\n";

const PERMANENT_FLAGS: &'static [u8] = b"* OK [PERMANENTFLAGS (\\Deleted \\Seen \\*)]\r\n";
const NO_PERMANENT_FLAGS: &'static [u8] =
    b"* OK [PERMANENTFLAGS ()] No permanent flags permitted\r\n";

impl IMAPOp for Select {
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
        select_or_examine(tag, args, state, db, true).await
    }
}

pub(super) async fn select_or_examine(
    tag: &str,
    args: &str,
    mut state: crate::imap::IMAPState,
    db: std::sync::Arc<tokio::sync::Mutex<crate::database::DBClient>>,
    select: bool,
) -> anyhow::Result<(
    crate::imap::Response,
    crate::imap::IMAPState,
    crate::imap::ResponseInfo,
)> {
    let id = match state {
        //outlook client somehow submits 2 select commands??
        IMAPState::Authed(id) => id,
        IMAPState::Selected(x) => x.user_id,
        _ => {
            return Ok((
                vec!["* BAD Wrong state\r\n".as_bytes().to_vec()],
                state,
                ResponseInfo::Regular,
            ));
        }
    };
    let mut msg = args.split_whitespace();
    let mailbox = match msg.next().context("should provide mailbox name") {
        Err(_) => {
            let resp = format!("{} BAD missing arguments\r\n", tag)
                .as_bytes()
                .to_vec();
            return Ok((vec![resp], state, ResponseInfo::Regular));
        }
        Result::Ok(a) => a.chars().filter(|c| c != &'"').collect::<String>(),
    };
    let db = db.lock().await;

    let m_id = match db.get_mailbox_id(id, &mailbox).await {
        Err(x) => {
            let resp = format!("{} BAD {}\r\n", tag, x).as_bytes().to_vec();
            return Ok((vec![resp], state, ResponseInfo::Regular));
        }
        Result::Ok(a) => a,
    };

    let unix_time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .context("Time shouldn't go backwards")?;
    let seconds: u32 = unix_time.as_secs().try_into()?;

    let uid_validity = format!("* OK [UIDVALIDITY {}]\r\n", seconds)
        .as_bytes()
        .to_vec();

    let count = db
        .mail_count(Some(m_id))
        .await
        .context("mail_count failed")?;
    let count_string = format!("* {} EXISTS\r\n", count).as_bytes().to_vec();

    let expected_uid = db.next_uid().await;
    let expected_uid_string = format!("* OK [UIDNEXT {}]\r\n", expected_uid)
        .as_bytes()
        .to_vec();
    let final_tagged = if select {
        format!("{} OK [READ-WRITE] SELECT completed\r\n", tag)
            .as_bytes()
            .to_vec()
    } else {
        format!("{} OK [READ-ONLY] EXAMINE COMPLETED\r\n", tag)
            .as_bytes()
            .to_vec()
    };
    let permanent_flags = if select {
        PERMANENT_FLAGS
    } else {
        NO_PERMANENT_FLAGS
    };
    let mailbox_list = format!("* LIST () \"/\" {}\r\n", mailbox)
        .as_bytes()
        .to_vec();
    let response = vec![
        count_string,
        uid_validity,
        expected_uid_string,
        FLAGS.to_vec(),
        mailbox_list,
        permanent_flags.to_vec(),
        final_tagged,
    ];
    if select {
        state = IMAPState::Selected(SelectedState {
            read_only: false,
            user_id: id,
            mailbox_id: m_id,
        });
    } else {
        state = IMAPState::Selected(SelectedState {
            read_only: true,
            user_id: id,
            mailbox_id: m_id,
        })
    }
    Ok((response, state, ResponseInfo::Regular))
}
