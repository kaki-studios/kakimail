use std::ops::Deref;
use std::ops::RangeFrom;
use std::ops::RangeInclusive;
use std::ops::RangeToInclusive;
use std::str::FromStr;
use std::sync::Arc;

use anyhow::anyhow;
use anyhow::Ok;
use anyhow::Result;
use chrono::NaiveDate;
use chrono::NaiveTime;
use libsql_client::args;
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

    // TODO some info might be in next command like in append
    let mut search_args = parsing::imap::search(args)?;
    if search_args.return_opts.is_empty() {
        // "If no result option is specified or empty list of options is specified as "()", ALL is assumed"
        search_args.return_opts = vec![ReturnOptions::All];
    }
    let db_result = db
        .lock()
        .await
        .exec_search_query(search_args, selected_state.mailbox_id, uid)
        .await?;
    let response = vec![
        format!("* ESEARCH (TAG \"{}\") {}\r\n", tag, db_result)
            .as_bytes()
            .to_vec(),
        format!("{} OK SEARCH completed\r\n", tag).into(),
    ];

    Ok((response, state, ResponseInfo::Regular))
}

#[derive(Debug, Clone)]
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

///one hell of an enum!
#[derive(Debug, Clone)]
pub enum SearchKeys {
    SequenceSet(SequenceSet),
    All,
    Answered,
    Bcc(String),
    Before(NaiveDate),
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
    On(chrono::NaiveDate),
    Or(Box<(SearchKeys, SearchKeys)>),
    Seen,
    SentBefore(chrono::NaiveDate),
    SentOn(chrono::NaiveDate),
    SentSince(chrono::NaiveDate),
    Since(chrono::NaiveDate),
    Smaller(i64),
    Subject(String),
    Text(String),
    To(String),
    Uid(SequenceSet),
    Unanswered,
    Undeleted,
    Undraft,
    Unflagged,
    Unkeyword(IMAPFlags),
    Unseen,
}

impl FromStr for SearchKeys {
    type Err = anyhow::Error;
    ///has to be a string produced by parsing/imap.rs search()
    fn from_str(s: &str) -> std::prelude::v1::Result<Self, Self::Err> {
        let (start, end) = s.split_once(" ").unwrap_or((s, ""));
        let end = end.to_string();
        let result = match start.to_lowercase().as_str() {
            "all" => SearchKeys::All,
            "answered" => SearchKeys::Answered,
            "bcc" => SearchKeys::Bcc(end),
            "before" => {
                let datetime = chrono::NaiveDate::parse_from_str(&end, parsing::DATE_FMT)?;
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
            "not" => SearchKeys::Not(Box::new(SearchKeys::from_str(&end)?)),
            "on" => {
                let date = chrono::NaiveDate::parse_from_str(&end, parsing::DATE_FMT)?;
                SearchKeys::On(date)
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
                let datetime = chrono::NaiveDate::parse_from_str(&end, parsing::DATE_FMT)?;
                SearchKeys::SentBefore(datetime)
            }
            "senton" => {
                let datetime = chrono::NaiveDate::parse_from_str(&end, parsing::DATE_FMT)?;
                SearchKeys::SentOn(datetime)
            }
            "sentsince" => {
                let datetime = chrono::NaiveDate::parse_from_str(&end, parsing::DATE_FMT)?;
                SearchKeys::SentSince(datetime)
            }
            "since" => {
                let datetime = chrono::NaiveDate::parse_from_str(&end, parsing::IMAP_DATETIME_FMT)?;
                SearchKeys::Since(datetime)
            }
            "smaller" => SearchKeys::Smaller(i64::from_str(&end)?),
            "subject" => SearchKeys::Subject(end),
            "text" => SearchKeys::Text(end),
            "to" => SearchKeys::To(end),
            "uid" => SearchKeys::Uid(SequenceSet::from_str(&end)?),
            "unanswered" => SearchKeys::Unanswered,
            "undeleted" => SearchKeys::Undeleted,
            "undraft" => SearchKeys::Undraft,
            "unflagged" => SearchKeys::Unflagged,
            "unkeyword" => SearchKeys::Unkeyword(IMAPFlags::from_str(&end)?),
            "unseen" => SearchKeys::Unseen,
            //sequence set, has to be last
            s => {
                let sequence_set = SequenceSet::from_str(s)?;
                SearchKeys::SequenceSet(sequence_set)
            }
        };
        Ok(result)
    }
}

impl SearchKeys {
    pub fn to_sql_arg(&self, uid: bool) -> (String, Vec<Value>) {
        //NOTE using String instead of &'static str because of SearchKeys::Or(x)

        match self {
            //idk, match anything
            SearchKeys::All => ("data LIKE \"%\"".to_string(), args!().to_vec()),
            //idk if this is the right syntax
            SearchKeys::Text(s) => (
                "data LIKE ?".to_string(),
                args!(format!("%{}%", s)).to_vec(),
            ),
            SearchKeys::Header(x, y) => (
                "data REGEXP ?".to_string(),
                //regex god
                args!(format!(".*{}: .*{}.*", x, y)).to_vec(),
            ),
            //whatever, same as text even though not supposed to be
            SearchKeys::Body(s) => (
                "data REGEXP ?".to_string(),
                //basically the line cannot contain ':' (header fields always have it)
                //TODO: improve this, too strict
                args!(format!("^(?!.*:).*{}.*", s)).to_vec(),
            ),
            //the flags
            SearchKeys::Answered => (
                "flags LIKE ?".to_owned(),
                args!(IMAPFlags::Answered.to_string()).to_vec(),
            ),
            SearchKeys::Flagged => (
                "flags LIKE ?".to_owned(),
                args!(IMAPFlags::Flagged.to_string()).to_vec(),
            ),
            SearchKeys::Deleted => (
                "flags LIKE ?".to_owned(),
                args!(IMAPFlags::Deleted.to_string()).to_vec(),
            ),
            SearchKeys::Seen => (
                "flags LIKE ?".to_owned(),
                args!(IMAPFlags::Seen.to_string()).to_vec(),
            ),
            SearchKeys::Draft => (
                "flags LIKE ?".to_owned(),
                args!(IMAPFlags::Draft.to_string()).to_vec(),
            ),
            //unflags, FIX don't use replace
            SearchKeys::Unanswered => (
                "flags LIKE ?".to_owned(),
                args!(IMAPFlags::Answered.to_string().replace("1", "0")).to_vec(),
            ),
            SearchKeys::Unflagged => (
                "flags LIKE ?".to_owned(),
                args!(IMAPFlags::Flagged.to_string().replace("1", "0")).to_vec(),
            ),
            SearchKeys::Undeleted => (
                "flags LIKE ?".to_owned(),
                args!(IMAPFlags::Deleted.to_string().replace("1", "0")).to_vec(),
            ),
            SearchKeys::Unseen => (
                "flags LIKE ?".to_owned(),
                args!(IMAPFlags::Seen.to_string().replace("1", "0")).to_vec(),
            ),
            SearchKeys::Undraft => (
                "flags LIKE ?".to_owned(),
                args!(IMAPFlags::Draft.to_string().replace("1", "0")).to_vec(),
            ),
            SearchKeys::Not(s) => {
                let result = s.to_sql_arg(uid);
                let new_string = format!("NOT ({})", result.0);
                (new_string, result.1)
            }
            //test these please
            SearchKeys::Larger(n) => ("length(data) > ?".to_owned(), args!(*n).to_vec()),
            SearchKeys::Smaller(n) => ("length(data) < ?".to_owned(), args!(*n).to_vec()),
            SearchKeys::Or(b) => {
                let keys = b.deref();
                let (mut result, result2) = (keys.0.to_sql_arg(uid), keys.1.to_sql_arg(uid));
                result.1.extend(result2.1);
                result.0.extend(" OR ".chars());
                result.0.extend(result2.0.chars());
                //result1 holds the result
                result
            }
            SearchKeys::To(s) => (
                "data REGEXP ?".to_string(),
                //regex god
                args!(format!(".*To: .*{}.*", s)).to_vec(),
            ),
            SearchKeys::SequenceSet(s) => {
                let mut final_str = String::from("(");
                let mut final_args = vec![];
                for (i, val) in s.sequences.iter().enumerate() {
                    let (new_str, new_arg) = match val {
                        Sequence::Int(i) => ("seqnum = ?", args!(*i).to_vec()),
                        //idk
                        Sequence::RangeFull => ("1", args!().to_vec()),
                        Sequence::RangeTo(r) => ("seqnum <= ?", args!(r.end).to_vec()),
                        Sequence::RangeFrom(r) => ("seqnum >= ?", args!(r.start).to_vec()),
                        Sequence::Range(r) => (
                            "(seqnum <= ? AND seqnum >= ?)",
                            args!(*r.end(), *r.start()).to_vec(),
                        ),
                    };
                    final_str.push_str(new_str);
                    final_args.extend(new_arg);
                    if i != s.sequences.len() - 1 {
                        final_str.push_str(" OR ");
                    } else {
                        final_str.push(')');
                    }
                }

                (final_str, final_args)
            }
            SearchKeys::Bcc(s) => (
                "data REGEXP ?".to_string(),
                args!(format!(".*Bcc: .*{}.*", s)).to_vec(),
            ),
            SearchKeys::Before(s) => {
                let unix_seconds = s.and_time(NaiveTime::default()).timestamp();
                (
                    "unixepoch(date) < ?".to_string(),
                    args!(unix_seconds).to_vec(),
                )
            }
            SearchKeys::On(s) => {
                let unix_seconds = s.and_time(NaiveTime::default()).timestamp();
                (
                    "unixepoch(date) = ?".to_string(),
                    args!(unix_seconds).to_vec(),
                )
            }
            SearchKeys::Since(s) => {
                let unix_seconds = s.and_time(NaiveTime::default()).timestamp();
                (
                    "unixepoch(date) > ?".to_string(),
                    args!(unix_seconds).to_vec(),
                )
            }
            SearchKeys::SentBefore(s) => {
                let datetime_str = s.format(parsing::DB_DATETIME_FMT).to_string();
                //trust me bro
                (r#"rfc2822_to_iso8601(regex_capture('^Date: (?:\w{3}, )?(\d{2} \w{3} \d{4} \d{2}:\d{2}:\d{2} [+-]\d{4})$', data, 1)) < ?"#.to_owned(), args!(datetime_str).to_vec())
            }
            SearchKeys::SentSince(s) => {
                let datetime_str = s.format(parsing::DB_DATETIME_FMT).to_string();
                (r#"rfc2822_to_iso8601(regex_capture('^Date: (?:\w{3}, )?(\d{2} \w{3} \d{4} \d{2}:\d{2}:\d{2} [+-]\d{4})$', data, 1)) > ?"#.to_owned(), args!(datetime_str).to_vec())
            }
            SearchKeys::SentOn(s) => {
                //BIGGEST TODO: s is a NaiveDate, can't format it using DB_DATETIME_FMT. convert it
                //to unix seconds, and make yet another sqlite function to convert rfc2822 (without
                //timezone) to unix seconds (+change regex to ignore timezone!!)
                let datetime_str = s.format(parsing::DB_DATETIME_FMT).to_string();
                (r#"rfc2822_to_iso8601(regex_capture('^Date: (?:\w{3}, )?(\d{2} \w{3} \d{4} \d{2}:\d{2}:\d{2} [+-]\d{4})$', data, 1)) = ?"#.to_owned(), args!(datetime_str).to_vec())
            }

            SearchKeys::Uid(s) => {
                let mut final_str = String::from("(");
                let mut final_args = vec![];
                for (i, val) in s.sequences.iter().enumerate() {
                    let (new_str, new_arg) = match val {
                        Sequence::Int(i) => ("uid = ?", args!(*i).to_vec()),
                        Sequence::RangeFull => ("1", args!().to_vec()),
                        Sequence::RangeTo(r) => ("uid <= ?", args!(r.end).to_vec()),
                        Sequence::RangeFrom(r) => ("uid >= ?", args!(r.start).to_vec()),
                        Sequence::Range(r) => (
                            "(uid <= ? AND uid >= ?)",
                            args!(*r.end(), *r.start()).to_vec(),
                        ),
                    };
                    final_str.push_str(new_str);
                    final_args.extend(new_arg);
                    if i != s.sequences.len() - 1 {
                        final_str.push_str(" OR ");
                    } else {
                        final_str.push(')');
                    }
                }

                (final_str, final_args)
            }
            SearchKeys::Subject(s) => (
                "data REGEXP ?".to_string(),
                args!(format!(".*Subject: .*{}.*", s)).to_vec(),
            ),
            SearchKeys::Cc(s) => (
                "data REGEXP ?".to_string(),
                args!(format!(".*Cc: .*{}.*", s)).to_vec(),
            ),
            SearchKeys::From(s) => (
                "data REGEXP ?".to_string(),
                args!(format!(".*From: .*{}.*", s)).to_vec(),
            ),
            SearchKeys::Keyword(s) => ("flags LIKE ?".to_owned(), args!(s.to_string()).to_vec()),
            SearchKeys::Unkeyword(s) => {
                ("flags NOT LIKE ?".to_owned(), args!(s.to_string()).to_vec())
            }
        }
    }
}

#[derive(Debug, Clone)]
pub enum Sequence {
    Int(u32),
    RangeTo(RangeToInclusive<u32>),
    RangeFrom(RangeFrom<u32>),
    Range(RangeInclusive<u32>),
    RangeFull,
}

impl FromStr for Sequence {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> std::prelude::v1::Result<Self, Self::Err> {
        match s.split_once(":") {
            Some(tuple) => match tuple {
                ("*", "*") => Ok(Sequence::RangeFull),
                ("*", num_str) => {
                    let num = num_str.parse::<u32>()?;
                    Ok(Sequence::RangeTo(..=num))
                }
                (num_str, "*") => {
                    let num = num_str.parse::<u32>()?;
                    Ok(Sequence::RangeFrom(num..))
                }
                (num_str1, num_str2) => {
                    let num1 = num_str1.parse::<u32>()?;
                    let num2 = num_str2.parse::<u32>()?;
                    Ok(Sequence::Range(num1..=num2))
                }
            },
            None => {
                let num = s.parse::<u32>()?;
                Ok(Sequence::Int(num))
            }
        }
    }
}

impl Sequence {
    pub fn contains(&self, num: u32) -> bool {
        match self {
            Sequence::Int(n) => &num == n,
            Sequence::RangeTo(r) => r.contains(&num),
            Sequence::RangeFrom(r) => r.contains(&num),
            Sequence::Range(r) => r.contains(&num),
            Sequence::RangeFull => true,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SequenceSet {
    pub sequences: Vec<Sequence>,
}

impl SequenceSet {
    pub fn contains(&self, num: u32) -> bool {
        self.sequences.iter().any(|i| i.contains(num))
    }
}

impl FromStr for SequenceSet {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> std::prelude::v1::Result<Self, Self::Err> {
        Ok(SequenceSet {
            sequences: s
                .split(",")
                .map(Sequence::from_str)
                .collect::<Result<Vec<Sequence>>>()?,
        })
    }
}
