use std::{mem, str::FromStr};

use anyhow::{anyhow, Context, Result};
use hickory_resolver::config::{ResolverConfig, ResolverOpts};
use libsql_client::{args, Value};

use crate::{
    database::IMAPFlags,
    imap_op::search::{SearchKeys, Sequence, SequenceSet},
};

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

pub fn sequence_set_to_sql(input: SequenceSet, column_name: &str) -> (String, Vec<Value>) {
    let mut final_str = String::from("(");
    let mut final_args = vec![];
    for (i, val) in input.sequences.iter().enumerate() {
        let (new_str, new_arg) = match val {
            Sequence::Int(i) => (format!("{} = ?", column_name), args!(*i).to_vec()),
            //idk
            Sequence::RangeFull => ("1 = 1".to_string(), args!().to_vec()),
            Sequence::RangeTo(r) => (format!("{} <= ?", column_name), args!(r.end).to_vec()),
            Sequence::RangeFrom(r) => (format!("{column_name} >= ?"), args!(r.start).to_vec()),
            Sequence::Range(r) => (
                format!("({column_name} <= ? AND {column_name} >= ?)"),
                args!(*r.end(), *r.start()).to_vec(),
            ),
        };
        final_str.push_str(&new_str);
        final_args.extend(new_arg);
        if i != input.sequences.len() - 1 {
            final_str.push_str(" OR ");
        } else {
            final_str.push(')');
        }
    }

    (final_str, final_args)
}
