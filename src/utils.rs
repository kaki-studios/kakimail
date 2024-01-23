// pub struct EmailAddress {
//     name: String,
//     domain: Domain,
// }
//
// pub struct Domain {
//     name: String,
//     tld: String,
// }
// not used bc String work just fine... if the email address is invalid, we send an error anyways
//

use std::any;

use anyhow::{anyhow, Result};
use hickory_resolver::name_server::{GenericConnector, TokioRuntimeProvider};

pub struct DnsResolver {
    pub resolver: hickory_resolver::AsyncResolver<GenericConnector<TokioRuntimeProvider>>,
}

impl DnsResolver {
    pub async fn resolve_mx(&self, domain: &str) -> Result<std::net::IpAddr> {
        //FIXME: dirty code
        let result1 = self.resolver.mx_lookup(domain.to_owned() + ".").await?;
        let min = result1
            .iter()
            .min_by_key(|k| k.preference())
            .ok_or(anyhow!("no preference found"))?;
        let result = self.resolver.lookup_ip(min.exchange().to_string()).await?;
        tracing::info!("{:?}", result);
        let test2 = result
            .as_lookup()
            .records()
            .first()
            .ok_or(anyhow!("no records found"))?
            .clone();
        let test3 = test2
            .into_record_of_rdata()
            .into_data()
            .ok_or(anyhow!("no data found"))?
            .ip_addr()
            .ok_or(anyhow!("cant construct ip address"))?;
        tracing::info!("{:?}", test3);
        return Ok(test3);
    }
}
