use std::str::from_utf8;

use nom::{
    bytes::complete::{tag, take},
    character::complete::digit1,
    sequence::tuple,
    IResult,
};

//TODO this code is copied from: https://github.com/djc/tokio-imap/blob/main/imap-proto/src/parser/core.rs
//fix it:
//- use &str, not &[u8]
//- better, more simple parsing
//- learn nom

pub fn literal(input: &[u8]) -> IResult<&[u8], &[u8]> {
    let mut parser = tuple((tag(b"{"), number, tag(b"}"), tag("\r\n")));

    let (remaining, (_, count, _, _)) = parser(input)?;

    let (remaining, data) = take(count)(remaining)?;
    //huh?? this will consume the data iterator, making it empty on return??
    if !data.iter().all(|byte| is_char8(*byte)) {
        return Err(nom::Err::Error(nom::error::Error::new(
            remaining,
            nom::error::ErrorKind::Char,
        )));
    }

    Ok((remaining, data))
}
pub fn is_char8(i: u8) -> bool {
    i != 0
}

pub fn number(i: &[u8]) -> IResult<&[u8], u32> {
    let (i, bytes) = digit1(i)?;
    match from_utf8(bytes)
        .ok()
        .and_then(|s| u32::from_str_radix(s, 10).ok())
    {
        Some(v) => Ok((i, v)),
        None => Err(nom::Err::Error(nom::error::make_error(
            i,
            nom::error::ErrorKind::MapRes,
        ))),
    }
}
