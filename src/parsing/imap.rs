//only temoporary
#![allow(unused)]

use nom::{
    branch::alt,
    bytes::complete::{tag, take_until, take_while},
    character::complete::{alpha1, char, digit1, multispace1},
    multi::{separated_list0, separated_list1},
    sequence::{delimited, tuple},
    IResult,
};

use crate::imap_op::search::{ReturnOptions, SearchKeys};

//NOTE this code is copied from: https://github.com/djc/tokio-imap/blob/main/imap-proto/src/parser/core.rs

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
                    let (_, s) = take_until("\"")(string)?;
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
pub fn search(input: &str) -> IResult<&str, SearchArgs> {
    dbg!(&input);
    let return_opts_parser = separated_list1(tag(" "), alpha1::<&str, nom::error::Error<&str>>);
    let mut start_parser = delimited(tag("RETURN ("), return_opts_parser, tag(") "));

    let (args, return_opts) = match start_parser(input).ok() {
        Some((rest, opts)) => (rest, Some(opts)),
        None => (input, None),
    };
    println!("{:?}, {:?}", args, return_opts);
    //TODO support quotes
    let parsed_args = parse_list(args)?;
    let mut iterator = parsed_args.iter();
    let mut new_args = vec![];
    while let Some(arg) = iterator.next() {
        //TODO support other args
        match arg.to_lowercase().as_str() {
            "all" => new_args.push(arg.to_string()),
            "answered" => new_args.push(arg.to_string()),
            "bcc" => {
                //find a way to do this without cloning
                if let Some(x) = iterator.next() {
                    new_args.push([arg.clone(), x.clone()].join(" "))
                }
            }
            "before" => {
                if let Some(x) = iterator.next() {
                    new_args.push([arg.clone(), x.clone()].join(" "))
                }
            }
            _ => {
                //could be sequence set
                new_args.push(arg.to_string())
            }
        }
    }
    // println!("{:?}, {:?}", parsed_args, new_args);
    Ok(("", SearchArgs::new()))
}

pub struct SearchArgs {
    return_opts: Vec<ReturnOptions>,
    search_keys: Vec<SearchKeys>,
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

#[cfg(test)]
mod tests {
    use crate::parsing::imap::{literal, mailbox, number, parse_list, quoted, Mailbox};

    use super::search;

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
        search("RETURN (MIN) UNSEEN BCC test").ok();
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
}
