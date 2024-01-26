use anyhow::{anyhow, Result};
use hickory_resolver::{
    config::{ResolverConfig, ResolverOpts},
    name_server::{GenericConnector, TokioRuntimeProvider},
};

///a struct used to resolve dns so that given a domain, we can find its ip
pub struct DnsResolver {
    pub resolver: hickory_resolver::AsyncResolver<GenericConnector<TokioRuntimeProvider>>,
}

impl DnsResolver {
    ///this function gets the highest preference ip address from a domain
    pub async fn resolve_mx(&self, domain: &str) -> Result<std::net::IpAddr> {
        //FIXME: dirty code
        //lookup returns a list of records it maps to
        let lookup = self.resolver.mx_lookup(domain.to_owned() + ".").await?;
        //apparently lowest number is highest preference
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
        tracing::info!("mx lookup succesful for {}, ip is {:?}", domain, ip);
        return Ok(ip);
    }
    ///default resolver for tokio
    pub fn default_new() -> Self {
        Self {
            resolver: hickory_resolver::AsyncResolver::tokio(
                ResolverConfig::default(),
                ResolverOpts::default(),
            ),
        }
    }
}
