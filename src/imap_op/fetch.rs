use std::str::FromStr;

use anyhow::{anyhow, Context};
use mailparse::MailHeaderMap;

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
    let (uid, (date, (mail, flags))): (Vec<_>, (Vec<_>, (Vec<_>, Vec<_>))) = db
        .lock()
        .await
        .fetch(sequence_set, uid)?
        .iter()
        .cloned()
        .unzip();
    let parsed_mail: Vec<_> = mail
        .iter()
        .map(String::as_bytes)
        .flat_map(mailparse::parse_mail)
        .collect();
    let mut _final_vec: Vec<String> = vec![];
    for item in &fetch_args {
        for mail in &parsed_mail {
            match item {
                //TODO:
                //match the item, then extract info from parse_mail accordingly, format as
                //String, add to a vec

                //example
                //TODO: change DB_DATETIME_FMT to include timezone offset => "%Y-%m-%d %H:%M:%S%.3f%:z"
                //and the in database::fetch() return DateTime<FixedOffset> instead of String and
                //then here format it accordingly
                FetchArgs::InternalDate => _final_vec.extend(date.clone()),
                _ => {
                    mail.get_headers().get_first_header("Date");
                }
            };
        }
    }

    Err(anyhow!("not implemented"))
}
