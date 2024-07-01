use anyhow::anyhow;

use crate::{
    imap::{IMAPOp, IMAPState},
    parsing::{self, imap::FetchArgs},
};

pub struct Fetch;

impl IMAPOp for Fetch {
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
        fetch_or_uid(tag, args, state, db, false).await
    }
}

pub(crate) async fn fetch_or_uid(
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
    let IMAPState::Selected(_user) = state else {
        return Err(anyhow!("wrong state"));
    };
    let (sequence_set, fetch_args) = parsing::imap::fetch(args)?;
    //the most cursed type ever
    let (seqnums, (uids, (dates, (mail_vec, flags)))): (
        Vec<_>,
        (Vec<_>, (Vec<_>, (Vec<_>, Vec<_>))),
    ) = db
        .lock()
        .await
        .fetch(sequence_set, uid)?
        .iter()
        .cloned()
        .unzip();
    let parsed_mail: Vec<_> = mail_vec
        .iter()
        .map(String::as_bytes)
        .flat_map(mailparse::parse_mail)
        .collect();

    let mut _final_vec: Vec<String> = vec![];
    for ((((seqnum, uid), date), mail), flag) in seqnums
        .iter()
        .zip(uids)
        .zip(dates)
        .zip(parsed_mail)
        .zip(flags)
    {
        let mut temp_buf = format!("* {seqnum} FETCH (");
        for item in &fetch_args {
            let data = match item {
                //TODO:
                //match the item, then extract info from parse_mail accordingly, format as
                //String, add to a vec
                FetchArgs::InternalDate => {
                    format!("INTERNALDATE \"{}\"", date.format("%d-%b-%Y %H:%M:%S %z"))
                }
                FetchArgs::Uid => format!("UID {uid}"),
                _ => String::default(),
            };
            temp_buf.extend(data.chars());
        }
        temp_buf.extend(")\r\n".chars());
        _final_vec.push(temp_buf);
    }

    Err(anyhow!("not implemented"))
}
