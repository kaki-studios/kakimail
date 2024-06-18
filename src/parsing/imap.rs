use std::str::FromStr;

use nom::{
    branch::alt,
    bytes::complete::{tag, take_till, take_while},
    character::complete::{alpha1, char, digit1},
    combinator::{map, map_res, opt, rest},
    multi::{separated_list0, separated_list1},
    sequence::{delimited, pair, preceded, tuple},
    IResult,
};

use crate::imap_op::search::{ReturnOptions, SearchKeys};

//NOTE this code has taken inspiration from: https://github.com/djc/tokio-imap/blob/main/imap-proto/src/parser/core.rs

pub fn literal(input: &str) -> IResult<&str, u32> {
    delimited(char('{'), number, alt((tag("}"), tag("+}"))))(input)
}

pub fn number(i: &str) -> IResult<&str, u32> {
    let (i, num) = digit1(i)?;
    match u32::from_str_radix(num, 10).ok() {
        Some(v) => Ok((i, v)),
        None => Err(nom::Err::Error(nom::error::make_error(
            i,
            nom::error::ErrorKind::MapRes,
        ))),
    }
}

//`HELLO WORLD "QUOTED ELEMENT"` -> ["HELLO", "WORLD", "QUOTED ELEMENT"]
pub fn parse_list(list: &str) -> Result<Vec<String>, nom::Err<nom::error::Error<&str>>> {
    let mut iterator = list.split_whitespace();
    let mut new_vec = Vec::new();
    while let Some(elem) = iterator.next() {
        if elem.starts_with('"') {
            let (stripped, _) = char('"')(elem)?;
            let mut start = stripped.to_string();
            while let Some(string) = iterator.next() {
                if string.ends_with('"') {
                    // let (_, s) = take_until("\"")(string)?;
                    let s = string.strip_suffix("\"").ok_or(nom::Err::Error(
                        nom::error::Error::new(string, nom::error::ErrorKind::Fail),
                    ))?;
                    //clunky
                    start.extend(" ".chars());
                    start.extend(s.chars());
                    break;
                }
                start.extend(string.chars());
            }
            new_vec.push(start)
        } else {
            new_vec.push(elem.to_string())
        }
    }
    Ok(new_vec)
}

///parses the search command
///assumes "SEARCH" is already stripped from the start
pub fn search(input: &str) -> Result<SearchArgs, nom::Err<nom::error::Error<String>>> {
    //use alt() instead
    let return_opts_parser = separated_list1(tag(" "), alpha1::<&str, nom::error::Error<&str>>);
    let mut start_parser = delimited(tag("RETURN ("), return_opts_parser, tag(") "));

    let (args, return_opts) = match start_parser(input).ok() {
        Some((rest, opts)) => (rest, Some(opts)),
        None => (input, None),
    };
    println!("info: {:?}, {:?}", args, return_opts);
    let parsed_return_opts = return_opts
        .iter()
        .flatten()
        .map(|i| *i)
        .map(ReturnOptions::from_str)
        .flatten()
        .collect::<Vec<ReturnOptions>>();

    let parsed_args = parse_list(args)
        //wtf is this
        .map_err(|e| e.map(|e2| nom::error::Error::new(e2.input.to_string(), e2.code)))?;
    let mut iterator = parsed_args.iter();
    let mut new_args = vec![];
    while let Some(arg) = iterator.next() {
        match arg.to_lowercase().as_str() {
            "all" | "answered" | "deleted" | "draft" | "flagged" | "seen" | "unanswered"
            | "undeleted" | "undraft" | "unflagged" | "unseen" => new_args.push(arg.to_string()),

            "bcc" | "body" | "cc" | "from" | "keyword" | "larger" | "not" | "smaller"
            | "subject" | "text" | "to" | "uid" | "unkeyword" | "before" | "on" | "sentbefore"
            | "senton" | "sentsince" | "since" => {
                //find a way to do this without cloning
                if let Some(x) = iterator.next() {
                    new_args.push([arg.clone(), x.clone()].join(" "))
                }
            }
            "header" | "or" => {
                //two strings
                if let Some(x) = iterator.next() {
                    if let Some(y) = iterator.next() {
                        //some illegal char, TODO: FIX
                        let rest = [x.clone(), y.clone()].join("`");
                        new_args.push([arg.clone(), rest].join(" "))
                    }
                }
            }
            _ => {
                //sequence set
                new_args.push(arg.to_string())
            }
        }
    }

    let searchkeys: Vec<SearchKeys> = new_args
        .iter()
        .map(|x| SearchKeys::from_str(x))
        .flatten()
        .collect();

    println!(
        "parsed_args: {:?}\nnew_args: {:?}\nsearchkeys: {:?}",
        parsed_args, new_args, searchkeys
    );
    Ok(SearchArgs {
        return_opts: parsed_return_opts,
        search_keys: searchkeys,
    })
}

#[derive(Debug, Clone)]
pub struct SearchArgs {
    pub return_opts: Vec<ReturnOptions>,
    pub search_keys: Vec<SearchKeys>,
}

impl SearchArgs {
    pub fn new() -> Self {
        SearchArgs {
            return_opts: vec![],
            search_keys: vec![],
        }
    }
}
//TODO quoted-specials should be able to be escaped
///removes the quotes
///RFC 9051:
/// quoted          = DQUOTE *QUOTED-CHAR DQUOTE
/// QUOTED-CHAR     = <any TEXT-CHAR except quoted-specials> /
///                   "\" quoted-specials / UTF8-2 / UTF8-3 / UTF8-4
/// quoted-specials = DQUOTE / "\"
pub fn quoted(input: &str) -> IResult<&str, &str> {
    let parse = take_while(|s| s != '"' && s != '\\');
    delimited(char('"'), parse, char('"'))(input)
}

/// string          = quoted / literal
pub fn string(input: &str) -> &str {
    match quoted(input).ok() {
        Some((_, result)) => result,
        None => input,
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Mailbox {
    Inbox,
    Custom(String),
}
impl From<&str> for Mailbox {
    fn from(s: &str) -> Self {
        if s.to_lowercase() == "inbox" {
            Mailbox::Inbox
        } else {
            Mailbox::Custom(s.to_string())
        }
    }
}

/// mailbox = "INBOX" / astring
///
/// INBOX is case-insensitive. All case variants of INBOX (e.g., "iNbOx")
/// MUST be interpreted as INBOX not as an astring.
pub fn mailbox(input: &str) -> IResult<&str, Mailbox> {
    nom::combinator::map(quoted, Mailbox::from)(input)
}
pub fn fetch_args(args: &str) -> IResult<&str, Vec<FetchArgs>> {
    //TODO: support macros:
    // ALL
    //     Macro equivalent to: (FLAGS INTERNALDATE RFC822.SIZE ENVELOPE)
    // FAST
    //     Macro equivalent to: (FLAGS INTERNALDATE RFC822.SIZE)
    // FULL
    //     Macro equivalent to: (FLAGS INTERNALDATE RFC822.SIZE ENVELOPE BODY)
    let mut x = delimited(
        opt(tag::<&str, &str, nom::error::Error<&str>>("(")),
        separated_list0(char(' '), alt((take_till(|i| i == ' ' || i == ')'), rest))),
        opt(tag(")")),
    );
    let result = x(args)?
        .1
        .iter()
        .map(|i| *i)
        .flat_map(FetchArgs::from_str)
        .map(|i| i.1)
        .collect::<Vec<_>>();
    dbg!(result);

    Ok(("", vec![]))
}

#[derive(Debug)]
pub enum FetchArgs {
    Binary(Vec<i32>, Option<(i32, i32)>),
    BinaryPeek(Vec<i32>, Option<(i32, i32)>),
    BinarySize(Vec<i32>),
    BodyNoArgs,
    Body(Vec<i32>, Option<(i32, i32)>),
    BodyPeek(Vec<i32>, Option<(i32, i32)>),
    BodyStructure,
    Envelope,
    Flags,
    InternalDate,
    RFC822Size,
    Uid,
}

impl FetchArgs {
    fn from_str(s: &str) -> IResult<&str, Self> {
        let mut word_parser = alt((
            take_till(|i| i == '['),
            rest::<&str, nom::error::Error<&str>>,
        ));

        let (arg_rest, word) = word_parser(s)?;

        //common parsers
        let section_part_parser = separated_list1(
            tag::<&str, &str, nom::error::Error<&str>>("."),
            map_res(digit1, str::parse::<i32>),
        );
        let mut section_binary_parser = delimited(char('['), section_part_parser, char(']'));

        let partial_parser = nom::sequence::separated_pair(
            map_res(digit1, str::parse::<i32>),
            char('.'),
            map_res(digit1, str::parse::<i32>),
        );
        let partial_parser_full = delimited(char('<'), partial_parser, char('>'));

        let result = match word.to_lowercase().as_str() {
            x if x == "binary" || x == "binary.peek" => {
                let mut full_parser = tuple((&mut section_binary_parser, opt(partial_parser_full)));
                let (rest, list) = full_parser(arg_rest)?;
                if x == "binary" {
                    (rest, FetchArgs::Binary(list.0, list.1))
                } else {
                    (rest, FetchArgs::BinaryPeek(list.0, list.1))
                }
            }
            "binary.size" => {
                let (rest, list) = section_binary_parser(arg_rest)?;
                (rest, FetchArgs::BinarySize(list))
            }
            "body" if arg_rest.is_empty() => ("", FetchArgs::BodyNoArgs),
            "body" => {
                //TODO: make new enum "Section" and parse `arg_rest` into Vec<Section>
                ("", FetchArgs::Uid)
            }

            _ => ("", FetchArgs::Uid),
        };
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use crate::{
        imap_op::{
            self,
            search::{self, Sequence},
        },
        parsing::{
            self,
            imap::{literal, mailbox, number, parse_list, quoted, Mailbox},
        },
    };

    use super::{fetch_args, search};
    use crate::imap_op::search::SequenceSet;

    #[test]
    fn test_literal() {
        assert_eq!(literal("{2}ok"), Ok(("ok", 2)));
        assert_eq!(literal("{2+}ok"), Ok(("ok", 2)))
    }
    #[test]
    fn test_number() {
        assert_eq!(number("23nme"), Ok(("nme", 23)))
    }
    #[test]
    fn test_quoted() {
        assert_eq!(
            quoted("\"test, 这不是ASCII\""),
            Ok(("", "test, 这不是ASCII"))
        )
    }
    #[test]
    fn test_mailbox() {
        assert_eq!(mailbox("\"iNbOx\""), Ok(("", Mailbox::Inbox)));
        assert_eq!(
            mailbox("\"not\""),
            Ok(("", Mailbox::Custom("not".to_string())))
        )
    }
    #[test]
    fn test_search() {
        let s = search(
            "RETURN (MIN MAX) UNSEEN BCC test TEXT \"some text\" SENTON 02-Oct-2020 2,3,7:10,15:*",
        )
        .unwrap();
        dbg!(&s);
        // let x = s.search_keys.iter().fold(String::new(), |mut acc, i| {
        //     let j = i.to_sql_arg();
        //     dbg!(&j);
        //     acc.extend(j.0.chars());
        //     acc
        // });
        // dbg!(x);
        let (_string, _values) = crate::database::DBClient::get_search_query(s, 0, false).unwrap();
    }
    #[test]
    fn test_list() {
        let result: Vec<String> = vec!["HELLO", "WORLD", "QUOTED ELEMENT"]
            .iter()
            .map(|e| e.to_string())
            .collect();
        assert_eq!(
            result,
            parse_list("HELLO WORLD \"QUOTED ELEMENT\"").unwrap()
        )
    }
    #[test]
    fn test_sequence_set() {
        let test_str = "2,3,7:10,15:*";
        let result = search::SequenceSet::from_str(test_str).unwrap();
        let expected = SequenceSet {
            sequences: vec![
                Sequence::Int(2),
                Sequence::Int(3),
                Sequence::Range(7..=10),
                Sequence::RangeFrom(15..),
            ],
        };
        assert_eq!(result, expected)
    }
    #[test]
    #[should_panic]
    fn test_bad_sequence_set() {
        let test_str = "2,3,7:10,1s:*";
        search::SequenceSet::from_str(test_str).unwrap();
    }
    #[test]
    fn test_sequence_set_from_list() {
        let list = vec![1, 2, 3, 5, 6, 7, 9, 11, 12];
        let result = SequenceSet::from(list);
        let expected = SequenceSet {
            sequences: vec![
                Sequence::Range(1..=3),
                Sequence::Range(5..=7),
                Sequence::Int(9),
                Sequence::Range(11..=12),
            ],
        };
        assert_eq!(result, expected)
    }
    #[test]
    fn test_date_regex() {
        let test_string = "To: test@example.com
Date: 01 Jan 2023 23:59:59 +0000
Subject: test";

        let matches =
            crate::database::regex_capture(imap_op::search::DATE_HEADER_REGEX, test_string, 1)
                .unwrap();
        let parsed_date = crate::database::rfc2822_to_date(&matches).unwrap();
        let naivedate = chrono::NaiveDate::from_ymd_opt(2024, 1, 1)
            .unwrap()
            .format(parsing::DB_DATE_FMT)
            .to_string();
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        let mut stmt = conn.prepare("SELECT 1 WHERE ? < ?").unwrap();
        let rows = stmt
            .query_map([parsed_date.clone(), naivedate.clone()], |i| {
                Ok(i.get::<_, i32>(0).unwrap())
            })
            .unwrap();
        let rows = rows.flatten().collect::<Vec<_>>();
        assert_eq!(rows[0], 1);
    }
    #[test]
    fn test_fetch() {
        fetch_args("(BINARY[1.2]<5.10> UID)").unwrap();
        fetch_args("BODY[]").unwrap();
    }
}
