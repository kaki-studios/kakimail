use std::str::FromStr;

use anyhow::{anyhow, Context, Ok};

use crate::{
    database::{IMAPFlags, StoreMode},
    imap::{IMAPOp, IMAPState, ResponseInfo},
    imap_op::search::SequenceSet,
};

pub struct Append;

impl IMAPOp for Append {
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
        let id = match state {
            IMAPState::Authed(id) => id,
            IMAPState::Selected(ref x) => x.user_id,
            _ => return Err(anyhow!("bad state")),
        };
        let Some(parsed) = parse_append_args(args)? else {
            let resp = b"+ Ready for literal data\r\n".to_vec();
            return Ok((vec![resp], state, ResponseInfo::RedoForNextMsg));
        };

        let mailbox_id = db
            .lock()
            .await
            .get_mailbox_id(id, &parsed.mailbox_name)
            .await?;

        let mut recipients = vec![];
        let mut from = String::new();
        for line in parsed.mail_data.lines() {
            match line.split_once(": ") {
                Some(("From", x)) => {
                    let start_index = x.find('<').map(|e| e + 1);
                    let end_index = x.find('>');
                    let indices = start_index.zip(end_index).unwrap_or((0, x.len()));
                    from = x[indices.0..indices.1].to_string();
                }
                Some(("To", x)) => recipients.push(x.to_string()),
                _ => {}
            }
        }
        let mail = crate::smtp_common::Mail {
            from,
            to: recipients,
            data: parsed.mail_data,
        };
        let new_uid = db
            .lock()
            .await
            .replicate(mail, mailbox_id, parsed.datetime)
            .await?;
        if !parsed.flags.is_empty() {
            db.lock().await.store_flags(
                mailbox_id,
                SequenceSet::from(vec![new_uid]),
                true,
                StoreMode::Add,
                &parsed.flags,
            )?;
        }
        let resp = format!(
            "{} OK [APPENDUID {} {}] APPEND completed\r\n",
            tag,
            db.lock().await.mailbox_uidvalidity(mailbox_id),
            new_uid
        );
        Ok((vec![resp.as_bytes().to_vec()], state, ResponseInfo::Regular))
    }
}

struct ParsedAppend {
    mailbox_name: String,
    flags: Vec<IMAPFlags>,
    datetime: Option<chrono::DateTime<chrono::FixedOffset>>,
    mail_data: String,
}

fn parse_append_args(args: &str) -> anyhow::Result<Option<ParsedAppend>> {
    let literal_start = args.find('{').context("should provide message literal")?;
    let literal_end = args[literal_start..]
        .find('}')
        .map(|idx| literal_start + idx)
        .context("bad literal marker")?;
    let count = args[literal_start + 1..literal_end]
        .chars()
        .filter(|c| c.is_ascii_digit())
        .collect::<String>()
        .parse::<usize>()?;
    if args[literal_end + 1..].trim().is_empty() || args.len() < count {
        return Ok(None);
    }
    let mail_data = args[args.len() - count..].to_string();
    let mut rest = args[..literal_start].trim();
    let (mailbox_name, new_rest) = take_imap_string(rest).context("missing mailbox")?;
    rest = new_rest.trim_start();

    let mut flags = Vec::new();
    let mut datetime = None;
    while !rest.is_empty() {
        if rest.starts_with('(') {
            let end = rest.find(')').context("unterminated flag list")?;
            let inner = &rest[1..end];
            flags = crate::parsing::imap::parse_list(inner)
                .map_err(|e| anyhow!("bad APPEND flag list: {:?}", e))?
                .into_iter()
                .map(|flag| IMAPFlags::from_str(&flag))
                .collect::<anyhow::Result<Vec<_>>>()?;
            rest = rest[end + 1..].trim_start();
        } else if rest.starts_with('"') {
            let (date_raw, new_rest) = take_imap_string(rest).context("bad APPEND date")?;
            datetime =
                chrono::DateTime::parse_from_str(&date_raw, crate::parsing::IMAP_DATETIME_FMT)
                    .map_err(|e| tracing::error!("error parsing datetime in append: {}", e))
                    .ok();
            rest = new_rest.trim_start();
        } else {
            break;
        }
    }

    Ok(Some(ParsedAppend {
        mailbox_name,
        flags,
        datetime,
        mail_data,
    }))
}

fn take_imap_string(input: &str) -> Option<(String, &str)> {
    if input.starts_with('"') {
        let mut escaped = false;
        let mut result = String::new();
        for (idx, c) in input[1..].char_indices() {
            if escaped {
                result.push(c);
                escaped = false;
                continue;
            }
            match c {
                '\\' => escaped = true,
                '"' => return Some((result, &input[idx + 2..])),
                _ => result.push(c),
            }
        }
        None
    } else {
        let end = input.find(char::is_whitespace).unwrap_or(input.len());
        Some((input[..end].to_string(), &input[end..]))
    }
}
