use std::str::FromStr;

use anyhow::{anyhow, Context};
use mailparse::MailHeaderMap;

use crate::{
    imap::{IMAPOp, IMAPState},
    parsing::{self},
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
        let IMAPState::Selected(_user) = state else {
            return Err(anyhow!("wrong state"));
        };
        let (sequence_set, fetch_args) = parsing::imap::fetch(args)?;
        let mail = db.lock().await.fetch(sequence_set)?;
        let parsed_mail: Vec<_> = mail
            .iter()
            .map(String::as_bytes)
            .flat_map(mailparse::parse_mail)
            .collect();
        let mut _final_vec: Vec<String> = vec![];
        for item in &fetch_args {
            for mail in &parsed_mail {
                match item {
                    //example
                    //TODO:
                    //match the item, then extract info from parse_mail accordingly, format as
                    //String, add to a vec
                    _ => mail.get_headers().get_first_header("Date"),
                };
            }
        }

        Err(anyhow!("not implemented"))
    }
}
