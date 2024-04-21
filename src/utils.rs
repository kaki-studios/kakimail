use std::{mem, str::FromStr};

use anyhow::{anyhow, Context, Result};
use hickory_resolver::config::{ResolverConfig, ResolverOpts};

use crate::{database::IMAPFlags, imap_op::search::SearchKeys};

pub const DECODER: base64::engine::GeneralPurpose = base64::engine::GeneralPurpose::new(
    &base64::alphabet::STANDARD,
    base64::engine::GeneralPurposeConfig::new(),
);

///a struct used to resolve dns so that given a domain, we can find its ip
pub struct DnsResolver {
    pub resolver: hickory_resolver::TokioAsyncResolver,
}

impl DnsResolver {
    ///this function gets the highest preference ip address from a domain
    pub async fn resolve_mx(&self, domain: &str) -> Result<std::net::IpAddr> {
        //TODO: dirty code
        //lookup returns a list of records it maps to
        let lookup = self.resolver.mx_lookup(domain.to_owned() + ".").await?;
        //apparently lowest number is highest preference, this is why it's sometimes called
        //"distance"
        let min = lookup
            .iter()
            .min_by_key(|k| k.preference())
            .ok_or(anyhow!("no preference found"))?;
        //the record is an A or AAAA record so we need to look it up too to get the ip
        let result = self.resolver.lookup_ip(min.exchange().to_string()).await?;
        let ip = result
            .as_lookup()
            .records()
            .first()
            .ok_or(anyhow!("no records found"))?
            .clone()
            .into_record_of_rdata()
            .into_data()
            .ok_or(anyhow!("no data found"))?
            .ip_addr()
            .ok_or(anyhow!("could not construct ip address"))?;
        return Ok(ip);
    }
    ///default resolver for tokio
    pub fn default_new() -> Self {
        Self {
            resolver: hickory_resolver::TokioAsyncResolver::tokio(
                ResolverConfig::default(),
                ResolverOpts::default(),
            ),
        }
    }
}

pub fn seperate_login(input: Vec<u8>) -> Result<(String, String)> {
    let mut strings = input
        .strip_prefix(b"\0")
        .ok_or(anyhow::anyhow!("auth error"))?
        .split(|n| n == &0);
    let usrname_b = strings.next().ok_or(anyhow::anyhow!("no password"))?;
    let usrname = String::from_utf8(usrname_b.to_vec())?;
    let password_b = strings.next().ok_or(anyhow::anyhow!("no password"))?;
    let password = String::from_utf8(password_b.to_vec())?;
    Ok((usrname, password))
}

pub fn parse_search_args<'a>(
    mut msg: impl Iterator<Item = &'a str> + Clone,
    msg_count: i64,
) -> Result<Vec<SearchKeys>> {
    let mut arg_vec = vec![];
    while let Some(arg) = msg.next() {
        let search_arg = match arg.to_lowercase().as_str() {
            "all" => SearchKeys::All,
            "answered" => SearchKeys::Answered,
            "bcc" => {
                //TODO this won't work if the search command spans over many requests
                //e.g. `
                // C: A285 SEARCH CHARSET UTF-8 TEXT {6}
                // S: + Ready for literal text
                // C: some
                //`
                let rest = msg
                    .clone()
                    .take_while(|x| !x.ends_with("\""))
                    .map(|x| x.chars().filter(|c| c != &'"').collect::<String>())
                    .collect::<Vec<_>>()
                    .join(" ");
                dbg!(&rest);

                SearchKeys::Bcc(rest)
            }
            x if x.contains(",") => {
                let mut sequence_set = vec![];
                for num in x.split(",") {
                    if num.contains(":") {
                        let (start_str, end_str) =
                            num.split_once(":").context("should be splittable")?;
                        let start = start_str.parse::<i64>().ok();
                        let end = end_str.parse::<i64>().ok();
                        let range = match (start, end) {
                            (Some(x), Some(y)) => x..=y,
                            (Some(x), None) => x..=msg_count,
                            (None, Some(y)) => 0..=y,
                            (None, None) => 0..=msg_count,
                        };
                        dbg!(&range);
                        sequence_set.extend(range)
                    } else {
                        if let Result::Ok(n) = num.parse::<i64>() {
                            sequence_set.push(n)
                        }
                    }
                }

                SearchKeys::SequenceSet(sequence_set)
            }
            "body" => {
                //dirty, check bcc for idea for improvement
                let search_term = msg
                    .clone()
                    .take_while(|e| !e.ends_with("\""))
                    .map(|x| x.chars().filter(|c| c != &'"').collect::<String>())
                    .collect::<Vec<_>>()
                    .join(" ");
                SearchKeys::Body(search_term)
            }
            "cc" => {
                //dirty, check bcc for idea for improvement
                let search_term = msg
                    .clone()
                    .take_while(|e| !e.ends_with("\""))
                    .map(|x| x.chars().filter(|c| c != &'"').collect::<String>())
                    .collect::<Vec<_>>()
                    .join(" ");
                SearchKeys::Cc(search_term)
            }

            "deleted" => SearchKeys::Deleted,
            "draft" => SearchKeys::Draft,
            "flagged" => SearchKeys::Flagged,
            "from" => {
                let search_term = msg
                    .clone()
                    .take_while(|e| !e.ends_with("\""))
                    .map(|x| x.chars().filter(|c| c != &'"').collect::<String>())
                    .collect::<Vec<_>>()
                    .join(" ");
                SearchKeys::From(search_term)
            }
            "header" => {
                let field_name = msg.next().context("should provide field name")?;
                let rest = msg
                    .clone()
                    .take_while(|e| !e.ends_with("\""))
                    .map(|x| x.chars().filter(|c| c != &'"').collect::<String>())
                    .collect::<Vec<_>>()
                    .join(" ");
                SearchKeys::Header(field_name.to_owned(), rest)
            }
            "keyword" => {
                let string = msg.next().context("should provide flag")?;
                let flag = IMAPFlags::from_str(string)?;
                SearchKeys::Keyword(flag)
            }
            "larger" => {
                let n_str = msg.next().context("should prodide size")?;
                let n = n_str.parse::<i64>()?;
                SearchKeys::Larger(n)
            }
            "not" => {
                //hope it doesn't recurse infinitely
                let mut vec = parse_search_args(msg.clone(), msg_count)?;
                if vec.is_empty() {
                    return Err(anyhow!("no search args found"));
                }
                //fighting the borrow checker, still a pretty ok solution
                let boxed = Box::new(mem::replace(&mut vec[0], SearchKeys::All));
                SearchKeys::Not(boxed)
            }
            "on" => {
                //TODO parse the date, format:
                //`
                // date            = date-text / DQUOTE date-text DQUOTE
                // date-day        = 1*2DIGIT
                //                     ; Day of month
                //
                // date-month      = "Jan" / "Feb" / "Mar" / "Apr" / "May" / "Jun" /
                //                   "Jul" / "Aug" / "Sep" / "Oct" / "Nov" / "Dec"
                //
                // date-text       = date-day "-" date-month "-" date-year
                //
                // date-year       = 4DIGIT
                //`
                return Err(anyhow!("not implemented"));
            }
            _ => continue,
        };
        arg_vec.push(search_arg)
    }
    Ok(arg_vec)
}
