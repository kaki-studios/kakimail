use anyhow::anyhow;
use mailparse::{MailAddr, MailHeader, MailHeaderMap, SingleInfo};

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
                FetchArgs::Envelope => {
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

                    format!("ENVELOPE ({date} {subject} {from} {sender} {reply_to} {to} {cc} {bcc} {in_reply_to} {message_id})")
                }
                FetchArgs::BodyNoArgs => {
                    //TODO

                    // tracing::debug!("mail with uid {uid} has {:?} parts", mail.parts().count());

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
    // for i in _final_vec {
    //     tracing::debug!("{i}");
    // }

    Err(anyhow!("not implemented"))
}

fn parenthesized_list_of_addr_structures(input: &mailparse::MailHeader) -> String {
    //TODO:
    //MailAddr::Single is pretty easy, but because MailAddr::Group is harder, i've made another
    //function get_data_single_item, so that inside the MailAddr::Group block i can call it in a
    //for loop. formatting the whole thing is still a pain in the ass because documentation is
    //obscure and available material is scarce. claude must help me. i just want to format the data
    //items correctly, how hard can it be?
    let mut final_buf = String::from("(");
    let _ = mailparse::addrparse_header(input).map(|i| {
        i.iter().for_each(|e| {
            let mut _display_name = String::from("NIL");
            let mut _groupname = String::from("NIL");
            let mut _username = String::from("NIL");
            let mut _domain = String::from("NIL");

            match e {
                MailAddr::Group(group) => {
                    // group.group_name
                    _groupname = format!("\"{}\"", group.group_name);
                }
                MailAddr::Single(single) => {
                    //TODO: format
                    dbg!(single);
                }
            }
            final_buf.push_str(format!("({_display_name} NIL {_username} {_domain})").as_str());
        });
    });
    // final_buf.push_str(format!("(\"{display_name}\" NIL \"{username}\" \"{domain}\")").as_str());
    final_buf.push(')');
    final_buf
}

fn get_data_single_item(input: &SingleInfo) -> String {
    let mut res = String::from("(");
    //TODO
    res.push(')');
    res
}
