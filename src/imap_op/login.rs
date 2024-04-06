use anyhow::{anyhow, Context, Ok};

use crate::imap::{IMAPOp, IMAPState, ResponseInfo};

pub struct Login;

impl IMAPOp for Login {
    async fn process(
        tag: &str,
        args: &str,
        mut state: crate::imap::IMAPState,
        db: std::sync::Arc<tokio::sync::Mutex<crate::database::DBClient>>,
    ) -> anyhow::Result<(Vec<Vec<u8>>, crate::imap::IMAPState, ResponseInfo)> {
        if state != IMAPState::NotAuthed {
            return Err(anyhow!("wrong state"));
        }
        let mut msg = args.split_whitespace();
        let mut username = msg.next().context("should provide username")?;
        let mut password = msg.next().context("should provice password")?;
        //NOTE: python's imaplib submits passwords enclosed like this: \"password\"
        //so we will need to remove them
        //NOTE: the raw_msg.split_whitespace() approach doesn't support passwords with spaces, but I think that's ok
        //for now
        password = &password[1..password.len() - 1];
        username = &username[1..username.len() - 1];
        let resp = if let Some(x) = db.lock().await.check_user(username, password).await {
            let good_msg = format!("{} OK LOGIN COMPLETED\r\n", tag);
            state = IMAPState::Authed(x);
            vec![good_msg.as_bytes().to_vec()]
        } else {
            let bad_msg = format!("{} NO LOGIN INVALID\r\n", tag);
            vec![bad_msg.as_bytes().to_vec()]
        };
        Ok((resp, state, ResponseInfo::Regular))
    }
}
