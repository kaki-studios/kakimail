use std::str::FromStr;

use anyhow::{anyhow, Context};

use crate::imap::{IMAPOp, IMAPState};

use super::search::SequenceSet;

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
        let (sequence_set_str, rest) = args
            .split_once(" ")
            .context(anyhow!("should always work"))?;
        let sequence_set = SequenceSet::from_str(sequence_set_str)?;
        let mail_list = db.lock().await.fetch(sequence_set)?;
        let parsed_mail_list = mail_list
            .iter()
            .flat_map(|mail| mailparse::parse_mail(mail.as_bytes()))
            .collect::<Vec<_>>();
        //TODO
        //parse `rest` into fetch args, get info from parsed_mail_list as appropriate, then return
        //fetched info

        Err(anyhow!("not implemented"))
    }
}
