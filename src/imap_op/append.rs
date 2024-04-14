use anyhow::{anyhow, Context, Ok};

use crate::imap::{IMAPOp, IMAPState, ResponseInfo};

pub struct Append;

const DATETIME_FMT: &'static str = "%d-%b-%y %H:%M:%S %z";
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
        let mut msg = args.split_whitespace();
        //saved-messages (\Seen) "datetime" {326} ....
        let _mailbox_name = msg.next().context("should provide mailbox name")?;
        //(\Seen) "datetime" {326} ....
        let mailbox_id = db.lock().await.get_mailbox_id(id, _mailbox_name).await?;

        let mut flags_raw = None;
        let mut datetime_raw = None;
        let mut count_raw = None;
        while let Some(arg) = msg.next() {
            if arg.len() == 0 {
                //just in case
                continue;
            }
            if count_raw.is_some() {
                //it's the last argument before the mail,
                //we should brak
            }
            match arg.chars().next().unwrap_or(' ') {
                '(' => {
                    flags_raw = Some(arg);
                }
                '{' => {
                    count_raw = Some(arg);
                    break;
                    //this is the last arg, we should break
                }
                '"' => {
                    //TODO this doesn't work
                    datetime_raw = Some(arg);
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
        mail_data = args[args.len() - count..].to_string();

        let mut datetime = None;
        if let Some(arg) = flags_raw {
            let _stripped = arg
                .strip_prefix("(")
                .context("should begin with (")?
                .strip_suffix(")")
                .context("should end with )")?;
            //the flags SHOULD be set in the resulting message...
            //TODO
        }
        if let Some(arg) = datetime_raw {
            let stripped_arg = arg.chars().filter(|c| c != &'"').collect::<String>();
            datetime = chrono::DateTime::parse_from_str(&stripped_arg, DATETIME_FMT)
                .map_err(|e| tracing::error!("{}", e))
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
            db.lock().await.biggest_uid().await.unwrap_or(0)
        );
        Ok((vec![resp.as_bytes().to_vec()], state, ResponseInfo::Regular))
    }
}
