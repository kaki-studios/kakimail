use std::str::FromStr;
use std::sync::Arc;

use anyhow::anyhow;
use anyhow::Context;
use anyhow::Ok;
use anyhow::Result;
use tokio::sync::Mutex;

use crate::database::IMAPFlags;
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
    let mut ret: Vec<ReturnOptions> = vec![];

    let mut charset: Option<&str> = None;
    let mut msg = args.split_whitespace();
    while let Some(arg) = msg.next() {
        if arg.starts_with("{") {
            //check the rfc if you don't know what this is for.
            //basically dirty parsing
            continue;
        }
        if arg.to_lowercase() == "charset" {
            charset = msg.next();
            if let Some(set) = charset {
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
        let search_arg = match arg.to_lowercase().as_str() {
            "all" => SearchArgs::All,
            "answered" => SearchArgs::Answered,
            "bcc" => {
                //TODO this won't work if the search command spans over many requests
                //e.g. `
                // C: A285 SEARCH CHARSET UTF-8 TEXT {12}
                // S: + Ready for literal text
                // C: отпуск
                //`
                let rest = msg
                    .clone()
                    .take_while(|x| !x.ends_with("\""))
                    .map(|x| x.chars().filter(|c| c != &'"').collect::<String>())
                    .collect::<Vec<_>>()
                    .join(" ");

                SearchArgs::Bcc(rest)
            }
            _ => continue,
        };
    }

    dbg!(ret);
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
enum SearchArgs {
    SequenceSet(String),
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
    Larger(i32),
    Not(String),
    On(String),
    Or(String, String),
    Seen,
    SentBefore(String),
    SentOn(String),
    SentSince(String),
    Since(String),
    Smaller(i32),
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
