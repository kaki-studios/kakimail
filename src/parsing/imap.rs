use nom::{
    bytes::complete::{escaped, take, take_while},
    character::complete::{alphanumeric0, char, digit1},
    sequence::{delimited, tuple},
    IResult,
};

//NOTE this code is copied from: https://github.com/djc/tokio-imap/blob/main/imap-proto/src/parser/core.rs

pub fn literal(input: &str) -> IResult<&str, &str> {
    //TODO change the functionality
    let mut parser = tuple((char('{'), number, char('}')));

    let (remaining, (_, count, _)) = parser(input)?;

    let (remaining, data) = take(count)(remaining)?;

    Ok((remaining, data))
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

#[cfg(test)]
mod tests {
    use crate::parsing::imap::{literal, number, quoted};

    #[test]
    fn test_literal() {
        assert_eq!(literal("{2}ok"), Ok(("", "ok")))
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
}
