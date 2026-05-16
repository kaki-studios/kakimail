use anyhow::{anyhow, Result};
use mailparse::{MailAddr, MailHeaderMap, ParsedMail, SingleInfo};

use crate::{
    database::{self, IMAPFlags, StoreMode},
    imap::{IMAPOp, IMAPState, ResponseInfo},
    parsing::{
        self,
        imap::{FetchArgs, SectionMsgText, SectionSpec, SectionText},
    },
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
    let (sequence_set, mut fetch_args) = parsing::imap::fetch(args)?;
    let include_uid = uid && !fetch_args.iter().any(|arg| matches!(arg, FetchArgs::Uid));
    if include_uid {
        fetch_args.insert(0, FetchArgs::Uid);
    }

    let should_mark_seen = !user.read_only
        && fetch_args.iter().any(|arg| {
            matches!(
                arg,
                FetchArgs::Body(_, _)
                    | FetchArgs::Binary(_, _)
                    | FetchArgs::RFC822
                    | FetchArgs::RFC822Text
            )
        });
    if should_mark_seen {
        db.lock().await.store_flags(
            user.mailbox_id,
            sequence_set.clone(),
            uid,
            StoreMode::Add,
            &[IMAPFlags::Seen],
        )?;
    }

    let rows = db.lock().await.fetch(sequence_set, uid, user.mailbox_id)?;

    let mut final_vec: Vec<Vec<u8>> = vec![];
    for row in rows {
        let mail = mailparse::parse_mail(row.data.as_bytes())?;
        let mut temp_buf = format!("* {} FETCH (", row.seqnum).into_bytes();
        for item in &fetch_args {
            match item {
                FetchArgs::InternalDate => temp_buf.extend_from_slice(
                    format!(
                        "INTERNALDATE \"{}\" ",
                        row.date.format("%d-%b-%Y %H:%M:%S %z")
                    )
                    .as_bytes(),
                ),
                FetchArgs::Uid => {
                    temp_buf.extend_from_slice(format!("UID {} ", row.uid).as_bytes())
                }
                FetchArgs::Flags => temp_buf.extend_from_slice(
                    format!(
                        "FLAGS ({}) ",
                        database::db_flag_to_readable_flag(&row.flags)
                    )
                    .as_bytes(),
                ),
                FetchArgs::RFC822 => append_named_literal(&mut temp_buf, "RFC822", mail.raw_bytes),
                FetchArgs::RFC822Header => {
                    let headers = headers_bytes(&mail, None, false);
                    append_named_literal(&mut temp_buf, "RFC822.HEADER", &headers);
                }
                FetchArgs::RFC822Size => temp_buf
                    .extend_from_slice(format!("RFC822.SIZE {} ", mail.raw_bytes.len()).as_bytes()),
                FetchArgs::RFC822Text => {
                    let body = mail.get_body_raw()?;
                    append_named_literal(&mut temp_buf, "RFC822.TEXT", &body);
                }
                FetchArgs::Envelope => {
                    temp_buf.extend_from_slice(envelope_to_string(&mail).as_bytes())
                }
                FetchArgs::BodyNoArgs => {
                    let bodystruct = super::bodystructure::build_bodystructure(&mail);
                    let result = super::bodystructure::bodystructure_to_string(&bodystruct, false);
                    temp_buf.extend_from_slice(format!("BODY {} ", result).as_bytes());
                }
                FetchArgs::BodyStructure => {
                    let bodystruct = super::bodystructure::build_bodystructure(&mail);
                    let result = super::bodystructure::bodystructure_to_string(&bodystruct, true);
                    temp_buf.extend_from_slice(format!("BODYSTRUCTURE {} ", result).as_bytes());
                }
                FetchArgs::Body(section, partial) | FetchArgs::BodyPeek(section, partial) => {
                    append_literal_fetch_item(&mut temp_buf, "BODY", section, *partial, &mail)?;
                }
                FetchArgs::Binary(section, partial) | FetchArgs::BinaryPeek(section, partial) => {
                    append_literal_fetch_item(
                        &mut temp_buf,
                        "BINARY",
                        &SectionSpec::Other(section.clone(), None),
                        *partial,
                        &mail,
                    )?;
                }
                FetchArgs::BinarySize(section) => {
                    let data = section_bytes(&SectionSpec::Other(section.clone(), None), &mail)?;
                    temp_buf.extend_from_slice(
                        format!("BINARY.SIZE[{}] {} ", render_part_path(section), data.len())
                            .as_bytes(),
                    );
                }
            }
        }
        while temp_buf.last() == Some(&b' ') {
            temp_buf.pop();
        }
        temp_buf.extend_from_slice(b")\r\n");
        final_vec.push(temp_buf);
    }
    final_vec.push(format!("{tag} OK FETCH completed\r\n").into_bytes());

    Ok((final_vec, state, ResponseInfo::Regular))
}

fn append_named_literal(buf: &mut Vec<u8>, name: &str, data: &[u8]) {
    buf.extend_from_slice(format!("{} {{{}}}\r\n", name, data.len()).as_bytes());
    buf.extend_from_slice(data);
    buf.push(b' ');
}

fn append_literal_fetch_item(
    buf: &mut Vec<u8>,
    name: &str,
    section: &SectionSpec,
    partial: Option<(i32, i32)>,
    mail: &ParsedMail,
) -> Result<()> {
    let mut data = section_bytes(section, mail)?;
    if let Some((start, count)) = partial {
        let start = start.max(0) as usize;
        let count = count.max(0) as usize;
        data = data.into_iter().skip(start).take(count).collect();
    }
    let rendered_section = render_section(section);
    buf.extend_from_slice(
        format!("{}[{}] {{{}}}\r\n", name, rendered_section, data.len()).as_bytes(),
    );
    buf.extend_from_slice(&data);
    buf.push(b' ');
    Ok(())
}

fn section_bytes(section: &SectionSpec, mail: &ParsedMail) -> Result<Vec<u8>> {
    match section {
        SectionSpec::MsgText(msgtext) => msgtext_bytes(msgtext, mail),
        SectionSpec::Other(parts, text) => {
            let target = select_part(mail, parts).unwrap_or(mail);
            match text {
                None => Ok(target.raw_bytes.to_vec()),
                Some(SectionText::Mime) => Ok(headers_bytes(target, None, false)),
                Some(SectionText::MsgText(msgtext)) => msgtext_bytes(msgtext, target),
            }
        }
    }
}

fn select_part<'a, 'b>(mail: &'b ParsedMail<'a>, parts: &[i32]) -> Option<&'b ParsedMail<'a>> {
    let mut current = mail;
    for part in parts {
        let idx = (*part).checked_sub(1)? as usize;
        current = current.subparts.get(idx)?;
    }
    Some(current)
}

fn msgtext_bytes(msgtext: &SectionMsgText, mail: &ParsedMail) -> Result<Vec<u8>> {
    match msgtext {
        SectionMsgText::Header => Ok(headers_bytes(mail, None, false)),
        SectionMsgText::HeaderFields(fields) => Ok(headers_bytes(mail, Some(fields), false)),
        SectionMsgText::HeaderFieldsNot(fields) => Ok(headers_bytes(mail, Some(fields), true)),
        SectionMsgText::Text => Ok(mail.get_body_raw()?),
    }
}

fn headers_bytes(mail: &ParsedMail, fields: Option<&[String]>, invert: bool) -> Vec<u8> {
    let normalized_fields = fields
        .map(|items| {
            items
                .iter()
                .map(|i| i.to_ascii_lowercase())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let mut result = Vec::new();
    for header in &mail.headers {
        let key = header.get_key();
        let selected = normalized_fields
            .iter()
            .any(|field| field.eq_ignore_ascii_case(&key));
        if fields.is_some() && selected == invert {
            continue;
        }
        result.extend_from_slice(format!("{}: {}\r\n", key, header.get_value()).as_bytes());
    }
    result.extend_from_slice(b"\r\n");
    result
}

fn render_part_path(parts: &[i32]) -> String {
    parts
        .iter()
        .map(i32::to_string)
        .collect::<Vec<_>>()
        .join(".")
}

fn render_section(section: &SectionSpec) -> String {
    match section {
        SectionSpec::MsgText(msgtext) => render_msgtext(msgtext),
        SectionSpec::Other(parts, None) if parts.is_empty() => String::new(),
        SectionSpec::Other(parts, None) => render_part_path(parts),
        SectionSpec::Other(parts, Some(SectionText::Mime)) => {
            join_part_and_text(parts, "MIME".to_string())
        }
        SectionSpec::Other(parts, Some(SectionText::MsgText(msgtext))) => {
            join_part_and_text(parts, render_msgtext(msgtext))
        }
    }
}

fn join_part_and_text(parts: &[i32], text: String) -> String {
    let path = render_part_path(parts);
    if path.is_empty() {
        text
    } else {
        format!("{}.{}", path, text)
    }
}

fn render_msgtext(msgtext: &SectionMsgText) -> String {
    match msgtext {
        SectionMsgText::Header => "HEADER".to_string(),
        SectionMsgText::HeaderFields(fields) => format!("HEADER.FIELDS ({})", fields.join(" ")),
        SectionMsgText::HeaderFieldsNot(fields) => {
            format!("HEADER.FIELDS.NOT ({})", fields.join(" "))
        }
        SectionMsgText::Text => "TEXT".to_string(),
    }
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
