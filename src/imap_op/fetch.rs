use anyhow::anyhow;
use mailparse::MailHeaderMap;

use crate::{
    database,
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
                    format!("INTERNALDATE \"{}\" ", date.format("%d-%b-%Y %H:%M:%S %z"))
                }
                FetchArgs::Uid => format!("UID {uid} "),
                FetchArgs::Flags => {
                    format!("FLAGS ({}) ", database::db_flag_to_readable_flag(&flag))
                }
                FetchArgs::RFC822Size => format!("RFC822.SIZE {} ", mail.raw_bytes.len()),
                FetchArgs::Envelope => {
                    let headers = mail.get_headers();
                    let date = headers.get_first_value("Date").unwrap_or("NIL".to_owned());
                    let subject = headers
                        .get_first_value("Subject")
                        .unwrap_or("NIL".to_owned());
                    let from = headers.get_first_value("From").unwrap_or("NIL".to_owned());
                    let sender = headers.get_first_value("Sender").unwrap_or(from.clone());
                    let reply_to = headers.get_first_value("Reply-To").unwrap_or(from.clone());
                    let to = headers.get_first_value("To").unwrap_or("NIL".to_owned());
                    let cc = headers.get_first_value("Cc").unwrap_or("NIL".to_owned());
                    let bcc = headers.get_first_value("Bcc").unwrap_or("NIL".to_owned());
                    let in_reply_to = headers.get_first_value("In-Reply-To");
                    let message_id = headers
                        .get_first_value("Message-ID")
                        .unwrap_or("NIL".to_owned());
                    //TODO: parse from, sender, reply-to, to, cc, and bcc fields into parenthesized lists of address structures.
                    //(check fetch response in rfc)
                    //others will be strings
                    //then concat all of them, and done

                    String::new()
                }
                _ => String::new(),
            };
            temp_buf.extend(data.chars());
        }
        //trim the trailing space
        temp_buf = temp_buf.trim_end().to_string();
        temp_buf.extend(")\r\n".chars());

        _final_vec.push(temp_buf);
    }
    tracing::debug!("{:?}", _final_vec);

    Err(anyhow!("not implemented"))
}
