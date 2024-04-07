use anyhow::{anyhow, Context};

use crate::imap::{IMAPOp, IMAPState, ResponseInfo};

pub struct List;

impl IMAPOp for List {
    async fn process(
        tag: &str,
        //should use the args
        _args: &str,
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
        //FIX this
        let mut mailboxes = db
            .lock()
            .await
            .get_mailbox_names_for_user(id)
            .await
            .context(anyhow!("couldn't get mailbox names"))?;
        mailboxes = mailboxes
            .iter()
            .map(|v| format!("* LIST () \"/\" {}\r\n", v))
            .collect();
        mailboxes.push(format!("{} OK LIST completed\r\n", tag));
        let mailboxes = mailboxes
            .iter()
            .map(|e| e.as_bytes().to_vec())
            .collect::<Vec<Vec<u8>>>();

        Ok((mailboxes, state, ResponseInfo::Regular))
    }
}
