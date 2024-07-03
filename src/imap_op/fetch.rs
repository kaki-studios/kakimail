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
    tracing::info!("seqnums: {:?}", seqnums);
    tracing::info!("uids: {:?}", uids);
    tracing::info!("dates: {:?}", dates);
    // tracing::info!("mail_vec: {:?}", seqnums);
    tracing::info!("flags: {:?}", flags);
    tracing::info!("----------------");

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
                    let date = headers
                        .get_first_value("Date")
                        .map(|i| format!("\"{i}\""))
                        .unwrap_or("NIL".to_owned());
                    let subject = headers
                        .get_first_value("Subject")
                        .map(|i| format!("\"{i}\""))
                        .unwrap_or("NIL".to_owned());

                    //make them into  parenthesized lists of address structures.
                    let mut from = headers.get_first_value("From").unwrap_or("NIL".to_owned());
                    tracing::debug!("From: {from}");
                    from = parenthesized_list_of_addr_structures(&from);

                    let mut sender = headers.get_first_value("Sender").unwrap_or(from.clone());
                    tracing::debug!("Sender: {sender}");
                    sender = parenthesized_list_of_addr_structures(&sender);

                    let mut reply_to = headers.get_first_value("Reply-To").unwrap_or(from.clone());
                    tracing::debug!("Reply-To: {reply_to}");
                    reply_to = parenthesized_list_of_addr_structures(&reply_to);

                    let mut to = headers.get_first_value("To").unwrap_or("NIL".to_owned());
                    tracing::debug!("To: {to}");
                    to = parenthesized_list_of_addr_structures(&to);

                    let mut cc = headers.get_first_value("Cc").unwrap_or("NIL".to_owned());
                    tracing::debug!("Cc: {cc}");
                    cc = parenthesized_list_of_addr_structures(&cc);

                    let mut bcc = headers.get_first_value("Bcc").unwrap_or("NIL".to_owned());
                    tracing::debug!("Bcc: {bcc}");
                    bcc = parenthesized_list_of_addr_structures(&bcc);

                    let in_reply_to = headers
                        .get_first_value("In-Reply-To")
                        .map(|i| format!("\"{i}\""))
                        .unwrap_or("NIL".to_owned());
                    let message_id = headers
                        .get_first_value("Message-ID")
                        .map(|i| format!("\"{i}\""))
                        .unwrap_or("NIL".to_owned());
                    tracing::debug!("----------------");
                    tracing::debug!("Date: {date}");
                    tracing::debug!("Subject: {subject}");
                    tracing::debug!("From: {from}");
                    tracing::debug!("Sender: {sender}");
                    tracing::debug!("Reply-To: {reply_to}");
                    tracing::debug!("To: {to}");
                    tracing::debug!("Cc: {cc}");
                    tracing::debug!("Bcc: {bcc}");
                    tracing::debug!("In-Reply-To: {in_reply_to}");
                    tracing::debug!("Message-ID: {message_id}");
                    tracing::debug!("----------------");

                    format!("ENVELOPE ({date} {subject} {from} {sender} {reply_to} {to} {cc} {bcc} {in_reply_to} {message_id})")
                }
                FetchArgs::BodyNoArgs => {
                    //TODO

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
    for i in _final_vec {
        tracing::debug!("{i}");
    }

    Err(anyhow!("not implemented"))
}

fn parenthesized_list_of_addr_structures(input: &str) -> String {
    let list = input.split(',');
    let mut final_buf = String::from("(");
    for i in list {
        if final_buf.as_str() != "(" {
            //if it's not the first iteration
            final_buf.push(' ')
        }
        if i.contains("<") {
            // it is of form "Display Name <username@domain.com>"
            let (display_name, rest) = i
                .split_once(" <")
                .map(|(i, rest)| (format!("\"{i}\""), rest))
                .unwrap_or(("NIL".to_owned(), i));

            let (username, rest) = rest
                .split_once("@")
                .map(|(i, rest)| (format!("\"{i}\""), rest))
                .unwrap_or(("NIL".to_owned(), rest));

            let domain = rest
                .strip_suffix(">")
                .map(|i| format!("\"{i}\""))
                .unwrap_or("NIL".to_owned());
            final_buf
                .push_str(format!("(\"{display_name}\" NIL \"{username}\" \"{domain}\")").as_str());
        } else {
            // it is of form "username@domain.com"
            let (username, domain) = i
                .split_once("@")
                .map(|(i, y)| (format!("\"{i}\""), format!("\"{y}\"")))
                .unwrap_or(("NIL".to_owned(), "NIL".to_owned()));

            final_buf.push_str(format!("(NIL NIL \"{username}\" \"{domain}\")").as_str())
        }
    }
    final_buf.extend(")".chars());
    final_buf
}
