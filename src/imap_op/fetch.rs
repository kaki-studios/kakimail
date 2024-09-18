use anyhow::anyhow;
use mailparse::{MailAddr, MailHeader, MailHeaderMap, ParsedMail, SingleInfo};

use crate::{
    database,
    imap::{IMAPOp, IMAPState, ResponseInfo},
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
    let IMAPState::Selected(user) = state else {
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
        .fetch(sequence_set, uid, user.mailbox_id)?
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
                FetchArgs::Envelope => envelope_to_string(&mail),
                FetchArgs::BodyNoArgs => {
                    let bodystruct = super::bodystructure::build_bodystructure(&mail);
                    let mut result =
                        super::bodystructure::bodystructure_to_string(&bodystruct, false);
                    result.push(' ');
                    result
                }
                FetchArgs::BodyStructure => {
                    let bodystruct = super::bodystructure::build_bodystructure(&mail);
                    let mut result =
                        super::bodystructure::bodystructure_to_string(&bodystruct, true);
                    result.push(' ');
                    result
                }
                FetchArgs::Body(sectionspec, opt) => {
                    //TODO
                    String::new()
                }
                FetchArgs::BinarySize(sects) => {
                    //NOTE: idk if this is correct
                    let mut temp = None;
                    for i in sects {
                        if temp.is_none() {
                            temp = mail.subparts.get(*i as usize);
                        } else {
                            temp = temp.map(|m| m.subparts.get(*i as usize)).flatten();
                        }
                    }
                    // temp.map(|i| i.raw_bytes.len()).unwrap_or(0);
                    dbg!(temp.map(|i| i.raw_bytes.len()).unwrap_or(0), temp);

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
    _final_vec.push(format!("{tag} OK FETCH completed\r\n"));
    // for i in &_final_vec {
    //     tracing::debug!("{i}");
    // }

    Ok((
        _final_vec
            .iter()
            .map(|i| i.as_bytes().to_vec())
            .collect::<Vec<_>>(),
        state,
        ResponseInfo::Regular,
    ))
}

fn parenthesized_list_of_addr_structures(input: &mailparse::MailHeader) -> String {
    let mut final_buf = String::from("(");
    let _ = mailparse::addrparse_header(input).map(|i| {
        i.iter().for_each(|e| match e {
            MailAddr::Group(group) => {
                final_buf.push_str(&format!("(NIL NIL {} NIL)", group.group_name));
                for i in &group.addrs {
                    final_buf.push_str(get_data_single_item(i).as_str());
                }
                final_buf.push_str("(NIL NIL NIL NIL)");
            }
            MailAddr::Single(single) => {
                final_buf.push_str(get_data_single_item(single).as_str());
            }
        });
    });
    final_buf.push(')');
    final_buf
}

///gets the data for a SingleInfo
fn get_data_single_item(i: &SingleInfo) -> String {
    let mut res = String::from("(");
    let (mbox_name, domain) = i.addr.split_once("@").unwrap_or(("NIL", "NIL"));
    let list = [
        i.display_name
            .clone()
            .map(|i| format!("\"{i}\""))
            .unwrap_or("NIL".to_owned())
            .as_str(),
        "NIL",
        &format!("\"{}\"", mbox_name),
        &format!("\"{}\"", domain),
    ]
    .join(" ");
    res.push_str(&list);
    res.push(')');
    res
}

pub fn envelope_to_string(mail: &ParsedMail) -> String {
    let headers = &mail.headers;
    let date = headers
        .get_first_value("Date")
        .map(|i| format!("\"{i}\""))
        .unwrap_or("NIL".to_owned());
    let subject = headers
        .get_first_value("Subject")
        .map(|i| format!("\"{i}\""))
        .unwrap_or("NIL".to_owned());

    //make them into  parenthesized lists of address structures.
    let from = headers
        .get_first_header("From")
        .map(parenthesized_list_of_addr_structures)
        .unwrap_or("NIL".to_owned());
    // tracing::debug!("From: {from}");

    let sender = headers
        .get_first_header("Sender")
        .map(parenthesized_list_of_addr_structures)
        .unwrap_or("NIL".to_owned());
    // tracing::debug!("Sender: {sender}");

    let reply_to = headers
        .get_first_header("Reply-To")
        .map(parenthesized_list_of_addr_structures)
        .unwrap_or("NIL".to_owned());
    // tracing::debug!("Reply-To: {sender}");

    let to = headers
        .get_first_header("To")
        .map(parenthesized_list_of_addr_structures)
        .unwrap_or("NIL".to_owned());
    // tracing::debug!("To: {to}");

    let cc = headers
        .get_first_header("Cc")
        .map(parenthesized_list_of_addr_structures)
        .unwrap_or("NIL".to_owned());
    // tracing::debug!("Cc: {cc}");

    let bcc = headers
        .get_first_header("Bcc")
        .map(parenthesized_list_of_addr_structures)
        .unwrap_or("NIL".to_owned());
    // tracing::debug!("Bcc: {bcc}");

    let in_reply_to = headers
        .get_first_value("In-Reply-To")
        .map(|i| format!("\"{i}\""))
        .unwrap_or("NIL".to_owned());
    let message_id = headers
        .get_first_value("Message-ID")
        .map(|i| format!("\"{i}\""))
        .unwrap_or("NIL".to_owned());
    // tracing::debug!("----------------");
    // tracing::debug!("Date: {date}");
    // tracing::debug!("Subject: {subject}");
    // tracing::debug!("From: {from}");
    // tracing::debug!("Sender: {sender}");
    // tracing::debug!("Reply-To: {reply_to}");
    // tracing::debug!("To: {to}");
    // tracing::debug!("Cc: {cc}");
    // tracing::debug!("Bcc: {bcc}");
    // tracing::debug!("In-Reply-To: {in_reply_to}");
    // tracing::debug!("Message-ID: {message_id}");
    // tracing::debug!("----------------");

    //trailing space is necessary
    format!("ENVELOPE ({date} {subject} {from} {sender} {reply_to} {to} {cc} {bcc} {in_reply_to} {message_id}) ")
}
