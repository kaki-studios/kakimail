//TODO
//-implement to_sql_arg()
//-do some stuff in database.rs
//-done!

use std::ops::Deref;
use std::str::FromStr;
use std::sync::Arc;

use anyhow::anyhow;
use anyhow::Context;
use anyhow::Ok;
use anyhow::Result;
use chrono::FixedOffset;
use libsql_client::Value;
use tokio::sync::Mutex;

use crate::database::IMAPFlags;
use crate::imap::IMAPState;
use crate::imap::ResponseInfo;
use crate::parsing;
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
pub enum ReturnOptions {
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
pub enum SearchKeys {
    SequenceSet(Vec<i64>),
    All,
    Answered,
    Bcc(String),
    Before(chrono::DateTime<FixedOffset>),
    Body(String),
    Cc(String),
    Deleted,
    Draft,
    Flagged,
    From(String),
    Header(String, String),
    Keyword(IMAPFlags),
    Larger(i64),
    Not(Box<SearchKeys>),
    On(chrono::DateTime<FixedOffset>),
    Or(Box<(SearchKeys, SearchKeys)>),
    Seen,
    //TODO change to DateTime
    SentBefore(chrono::DateTime<FixedOffset>),
    SentOn(chrono::DateTime<FixedOffset>),
    SentSince(chrono::DateTime<FixedOffset>),
    Since(chrono::DateTime<FixedOffset>),
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

impl FromStr for SearchKeys {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> std::prelude::v1::Result<Self, Self::Err> {
        let (start, end) = s.split_once(" ").unwrap_or((s, ""));
        let end = end.to_string();
        let result = match start.to_lowercase().as_str() {
            "all" => SearchKeys::All,
            "answered" => SearchKeys::Answered,
            "bcc" => SearchKeys::Bcc(end),
            "before" => {
                let datetime = chrono::DateTime::parse_from_str(&end, parsing::IMAP_DATETIME_FMT)?;
                SearchKeys::Before(datetime)
            }
            "body" => SearchKeys::Body(end),
            "cc" => SearchKeys::Cc(end),
            "deleted" => SearchKeys::Deleted,
            "draft" => SearchKeys::Draft,
            "flagged" => SearchKeys::Flagged,
            "from" => SearchKeys::From(end),
            "header" => {
                //see parsing/imap.rs
                let (fieldname, rest) = end
                    .split_once("`")
                    .ok_or(anyhow!("couldn't parse HEADER"))?;
                SearchKeys::Header(fieldname.to_string(), rest.to_string())
            }
            "keyword" => SearchKeys::Keyword(IMAPFlags::from_str(&end)?),
            "larger" => SearchKeys::Larger(i64::from_str(&end)?),
            "not" => SearchKeys::Keyword(IMAPFlags::from_str(&end)?),
            "on" => {
                let datetime = chrono::DateTime::parse_from_str(&end, parsing::IMAP_DATETIME_FMT);
                SearchKeys::On(datetime?)
            }
            "or" => {
                //let's hope it works, see parsing/imap.rs
                let (key1_str, key2_str) =
                    end.split_once("`").ok_or(anyhow!("couldn't parse OR"))?;
                let keys = (
                    SearchKeys::from_str(key1_str)?,
                    SearchKeys::from_str(key2_str)?,
                );
                SearchKeys::Or(Box::new(keys))
            }
            "seen" => SearchKeys::Seen,
            "sentbefore" => {
                let datetime = chrono::DateTime::parse_from_str(&end, parsing::IMAP_DATETIME_FMT)?;
                SearchKeys::SentBefore(datetime)
            }
            "senton" => {
                let datetime = chrono::DateTime::parse_from_str(&end, parsing::IMAP_DATETIME_FMT)?;
                SearchKeys::SentOn(datetime)
            }
            "sentsince" => {
                let datetime = chrono::DateTime::parse_from_str(&end, parsing::IMAP_DATETIME_FMT)?;
                SearchKeys::SentSince(datetime)
            }
            "since" => {
                let datetime = chrono::DateTime::parse_from_str(&end, parsing::IMAP_DATETIME_FMT)?;
                SearchKeys::Since(datetime)
            }
            "smaller" => SearchKeys::Smaller(i64::from_str(&end)?),
            "subject" => SearchKeys::Subject(end),
            "text" => SearchKeys::Text(end),
            "to" => SearchKeys::To(end),
            "uid" => SearchKeys::Uid(end),
            "unanswered" => SearchKeys::Unanswered,
            "undeleted" => SearchKeys::Undeleted,
            "undraft" => SearchKeys::Undraft,
            "unflagged" => SearchKeys::Unflagged,
            "unkeyword" => SearchKeys::Unkeyword(IMAPFlags::from_str(&end)?),
            "unseen" => SearchKeys::Unseen,

            _ => return Err(anyhow!("not implemented")),
        };
        Ok(result)
    }
}

impl SearchKeys {
    pub fn to_sql_arg(&self) -> (Vec<Value>, String) {
        //TODO
        let string = match self {
            //idk, match anything
            SearchKeys::All => "data LIKE \"%\"".to_string(),
            //TODO this is literally sql injection
            SearchKeys::Text(s) => format!("data LIKE \"%{}%\"", s),
            //headers are "x: y", right? +header can contain y anywhere
            SearchKeys::Header(x, y) => format!("data LIKE \"%{}: %{}%\"", x, y),
            //whatever, same as text even though not supposed to be
            SearchKeys::Body(s) => format!("data LIKE \"%{}%\"", s),
            //the flags
            SearchKeys::Answered => format!("flags LIKE {}", IMAPFlags::Answered.to_string()),
            SearchKeys::Flagged => format!("flags LIKE {}", IMAPFlags::Flagged.to_string()),
            SearchKeys::Deleted => format!("flags LIKE {}", IMAPFlags::Deleted.to_string()),
            SearchKeys::Seen => format!("flags LIKE {}", IMAPFlags::Seen.to_string()),
            SearchKeys::Draft => format!("flags LIKE {}", IMAPFlags::Draft.to_string()),
            //unflags, FIX don't use replace
            SearchKeys::Unanswered => format!(
                "flags LIKE {}",
                IMAPFlags::Answered.to_string().replace("1", "0")
            ),
            SearchKeys::Unflagged => format!(
                "flags LIKE {}",
                IMAPFlags::Flagged.to_string().replace("1", "0")
            ),
            SearchKeys::Undeleted => format!(
                "flags LIKE {}",
                IMAPFlags::Deleted.to_string().replace("1", "0")
            ),
            SearchKeys::Unseen => format!(
                "flags LIKE {}",
                IMAPFlags::Seen.to_string().replace("1", "0")
            ),
            SearchKeys::Undraft => format!(
                "flags LIKE {}",
                IMAPFlags::Draft.to_string().replace("1", "0")
            ),
            //somewhat scuffed
            // SearchKeys::Not(s) => s.to_string().replace("LIKE", "NOT LIKE"),
            //test these please
            SearchKeys::Larger(n) => format!("length(data) > {}", n),
            SearchKeys::Smaller(n) => format!("length(data) < {}", n),
            SearchKeys::Or(b) => {
                let keys = b.deref();
                let (mut result1, result2) = (keys.0.to_sql_arg(), keys.1.to_sql_arg());
                result1.0.extend(result2.0);
                result1.1.extend(result2.1.chars());
                //TODO convert every othes command to use (Vec<Value, String)
                //result1 holds the result
                String::from("error")
            }
            //TODO header keys (to, subject, etc)
            _ => "todo".to_string(),
        };
        (vec![], "".to_string())
    }
}
