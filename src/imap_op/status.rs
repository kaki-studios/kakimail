use anyhow::{anyhow, Context};

use crate::{
    database::IMAPFlags,
    imap::{IMAPOp, IMAPState, ResponseInfo},
};

pub struct Status;

impl IMAPOp for Status {
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
        let IMAPState::Authed(id) = state else {
            return Err(anyhow!("bad state"));
        };
        let mut msg = args.split_whitespace();
        let mailbox_name = msg.next().context("should provide mailbox name")?;

        //remove the parentheses (UIDNEXT MESSAGES) -> UIDNEXT MESSAGES
        let rest = msg
            .map(|m| m.chars().filter(|c| c.is_alphabetic()).collect::<String>())
            .collect::<Vec<_>>();
        let db = db.lock().await;
        let mailbox_id = db.get_mailbox_id(id, mailbox_name).await?;
        //hate this type
        let mut result: Vec<Vec<u8>> = vec![];

        dbg!(&rest);
        for attr in rest {
            match attr.as_str() {
                "MESSAGES" => {
                    let msg_count = db.mail_count(Some(mailbox_id)).await?;
                    result.push(format!("MESSAGES {}", msg_count).as_bytes().to_vec());
                }
                "UIDNEXT" => {
                    let nextuid = db.biggest_uid().await.unwrap_or(-1) + 1;
                    result.push(format!("UIDNEXT {}", nextuid).as_bytes().to_vec());
                }
                "UNSEEN" => {
                    let count = db
                        .mail_count_with_flags(mailbox_id, vec![(IMAPFlags::Seen, false)])
                        .await?;
                    result.push(format!("UNSEEN {}", count).as_bytes().to_vec());
                }
                "DELETED" => {
                    let count = db
                        .mail_count_with_flags(mailbox_id, vec![(IMAPFlags::Deleted, true)])
                        .await?;
                    result.push(format!("DELETED {}", count).as_bytes().to_vec());
                }
                "SIZE" => {
                    //TODO
                    //probably just do a sum() in sql, doesn't need to be accurate
                }
                _ => continue,
            }
        }
        let response1_raw = String::from_utf8(result.join(" ".as_bytes()))?;
        let response1 = format!("* STATUS {} ({})\r\n", mailbox_name, response1_raw)
            .as_bytes()
            .to_vec();
        let response2 = format!("{} OK STATUS completed\r\n", tag)
            .as_bytes()
            .to_vec();

        Ok((vec![response1, response2], state, ResponseInfo::Regular))
    }
}
