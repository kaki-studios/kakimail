use anyhow::{anyhow, Context, Ok};

use crate::imap::{IMAPOp, IMAPState, ResponseInfo};

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
        //TODO set flags and the parsed datetime in the final message
        let id = match state {
            IMAPState::Authed(id) => id,
            IMAPState::Selected(ref x) => x.user_id,
            _ => return Err(anyhow!("bad state")),
        };
        dbg!(&args);
        let mut msg = args.split_whitespace();
        //saved-messages (\Seen) "datetime" {326} ....
        let _mailbox_name = msg.next().context("should provide mailbox name")?;
        //(\Seen) "datetime" {326} ....
        let mailbox_id = db.lock().await.get_mailbox_id(id, _mailbox_name).await?;

        let mut flags_raw = None;
        let mut datetime_raw = None;
        let mut count_raw = None;
        while let Some(arg) = msg.next() {
            dbg!(&arg);
            let mut chars = arg.chars();
            match chars.next().unwrap_or(' ') {
                '(' => {
                    // TODO this might not work
                    flags_raw = Some(arg);
                }
                '{' => {
                    count_raw = Some(arg);
                    break;
                    //this is the last arg, we should break
                }
                '"' => {
                    let start = chars.collect::<String>();

                    let mut stop_next = false;
                    let middle = msg
                        .clone()
                        //very clunky but what can you do?
                        .map_while(move |i| {
                            if stop_next {
                                return None;
                            }
                            if i.ends_with("\"") {
                                stop_next = true;
                                return i.to_string().strip_suffix("\"").map(|x| x.to_string());
                            }
                            Some(i.to_string())
                        })
                        .collect::<Vec<String>>()
                        .join(" ");

                    tracing::debug!("parsed datetime is {}", start);
                    let datetime = [start, middle].join(" ");
                    tracing::debug!("datetime format is {}", crate::parsing::IMAP_DATETIME_FMT);
                    datetime_raw = Some(datetime);
                }
                _ => {}
            }
        }
        //dirty trick
        //(\Flag) "date"
        //into ["(\Flag", "\"date\""]
        let msg_size = count_raw.context("should provide message literal")?;
        let count = msg_size
            .chars()
            .filter(|c| c.is_digit(10))
            .collect::<String>();
        let count = count.parse::<usize>()?;
        let mail_data: String;
        if msg.next() == None {
            let resp = b"+ Ready for literal data\r\n".to_vec();
            return Ok((vec![resp], state, ResponseInfo::RedoForNextMsg));
        }
        tracing::debug!("args_len: {}", args.len());
        tracing::debug!("count: {}", count);
        mail_data = args[args.len() - count..].to_string();

        let mut datetime = None;
        if let Some(arg) = flags_raw {
            let _stripped = arg
                .strip_prefix("(")
                .context("should begin with (")?
                .strip_suffix(")")
                .context("should end with )")?;
            //the flags SHOULD be set in the stored message...
            //TODO
        }
        if let Some(arg) = datetime_raw {
            dbg!(&arg);
            datetime = chrono::DateTime::parse_from_str(&arg, crate::parsing::IMAP_DATETIME_FMT)
                .map_err(|e| tracing::error!("error parsing datetime in append: {}", e))
                .ok();
        }

        let mut recipients = vec![];
        let mut from = "".to_string();
        for line in mail_data.lines() {
            match line.split_once(": ") {
                Some(("From", x)) => {
                    let start_index = x.find("<").map(|e| e + 1);
                    let end_index = x.find(">");
                    let indices = start_index.zip(end_index).unwrap_or((0, 1));
                    from = x[indices.0..indices.1].to_string();
                }
                Some(("To", x)) => {
                    recipients.push(x.to_string());
                }
                _ => {}
            }
        }
        let mail = crate::smtp_common::Mail {
            from,
            to: recipients,
            data: mail_data,
        };
        db.lock()
            .await
            .replicate(mail, mailbox_id, datetime)
            .await?;
        let unix_time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .context("Time shouldn't go backwards")?;
        let seconds: u32 = unix_time.as_secs().try_into()?;
        let resp = format!(
            "{} OK [APPENDUID {} {}] APPEND completed\r\n",
            tag,
            seconds,
            db.lock().await.next_uid().await
        );
        Ok((vec![resp.as_bytes().to_vec()], state, ResponseInfo::Regular))
    }
}
