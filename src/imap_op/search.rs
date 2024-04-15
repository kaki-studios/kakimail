use std::str::FromStr;
use std::sync::Arc;

use anyhow::anyhow;
use anyhow::Context;
use anyhow::Ok;
use anyhow::Result;
use tokio::sync::Mutex;

use crate::database::IMAPFlags;
use crate::imap::IMAPState;
use crate::imap::ResponseInfo;
use crate::{
    database::DBClient,
    imap::{self, IMAPOp},
};

pub struct Search;

impl IMAPOp for Search {
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
        search_or_uid(tag, args, state, db, false).await
    }
}

pub(super) async fn search_or_uid(
    tag: &str,
    args: &str,
    state: crate::imap::IMAPState,
    db: Arc<Mutex<DBClient>>,
    uid: bool,
) -> Result<(imap::Response, imap::IMAPState, imap::ResponseInfo)> {
    let IMAPState::Selected(selected_state) = state else {
        return Err(anyhow!("bad state"));
    };

    let mut ret: Vec<ReturnOptions> = vec![];
    //used later
    let msg_count = db
        .lock()
        .await
        .mail_count(Some(selected_state.mailbox_id))
        .await?;

    let mut msg = args.split_whitespace();
    while let Some(arg) = msg.next() {
        if arg.starts_with("{") {
            //check the rfc if you don't know what this is for.
            //basically dirty parsing
            //probably should be used!!
            continue;
        }
        if arg.to_lowercase() == "charset" {
            if let Some(set) = msg.next() {
                if set.to_lowercase() != "utf-8" {
                    return Ok((
                        vec![format!("{} BAD unsupported charset", tag)
                            .as_bytes()
                            .to_vec()],
                        state,
                        ResponseInfo::Regular,
                    ));
                }
            }
            continue;
        }
        if arg.to_lowercase() == "return" {
            loop {
                let return_arg = msg.next().context("should provide next arg")?;
                let fitered = &return_arg
                    .chars()
                    .filter(|c| c.is_alphabetic())
                    .collect::<String>();
                let parse_result = ReturnOptions::from_str(fitered);
                let Result::Ok(parse) = parse_result else {
                    if return_arg.ends_with(")") {
                        break;
                    }
                    continue;
                };

                ret.push(parse);
                if return_arg.ends_with(")") {
                    break;
                }
            }
            if ret.is_empty() {
                ret.push(ReturnOptions::All)
            }
            continue;
        }
    }
    let arg_vec = crate::utils::parse_search_args(msg, msg_count)?;

    dbg!(ret);
    dbg!(arg_vec);

    Err(anyhow!("not implemented"))
}

#[derive(Debug)]
enum ReturnOptions {
    Min,
    Max,
    All,
    Count,
    Save,
}

impl FromStr for ReturnOptions {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> std::prelude::v1::Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "min" => Ok(ReturnOptions::Min),
            "max" => Ok(ReturnOptions::Max),
            "all" => Ok(ReturnOptions::All),
            "count" => Ok(ReturnOptions::Count),
            "save" => Ok(ReturnOptions::Save),
            x => Err(anyhow!("couldn't parse {} into a ReturnOptions", x)),
        }
    }
}

//TODO use dates
///one hell of an enum!
#[derive(Debug, Clone)]
pub enum SearchArgs {
    SequenceSet(Vec<i64>),
    All,
    Answered,
    Bcc(String),
    Body(String),
    Cc(String),
    Deleted,
    Draft,
    Flagged,
    From(String),
    Header(String, String),
    Keyword(IMAPFlags),
    Larger(i64),
    Not(Box<SearchArgs>),
    On(String),
    Or(String, String),
    Seen,
    SentBefore(String),
    SentOn(String),
    SentSince(String),
    Since(String),
    Smaller(i64),
    Subject(String),
    Text(String),
    To(String),
    Uid(String),
    Unanswered,
    Undeleted,
    Undraft,
    Unflagged,
    Unkeyword(IMAPFlags),
    Unseen,
}
