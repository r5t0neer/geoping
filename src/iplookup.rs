use std::cell::RefCell;
use std::collections::HashMap;
use std::time::Duration;
use ipinfo::{IpDetails, IpError, IpInfo, IpInfoConfig};
use crate::{IP, IPINFO_SECRET, IPINFO_TIMEOUT};

pub struct IpInfoClientWrapper
{
    native_client: IpInfo,
    cache: HashMap<IP, IpDetails>,
}

impl IpInfoClientWrapper
{
    pub fn new(secret: &str, query_timeout: Duration) -> Result<IpInfoClientWrapper, IpError>
    {
        let ipcfg = IpInfoConfig {
            token: Some(secret.to_string()),
            timeout: query_timeout,
            ..Default::default()
        };

        Ok(IpInfoClientWrapper {
            native_client: IpInfo::new(ipcfg)?,
            cache: HashMap::new()
        })
    }

    pub async fn query(&mut self, ip: &str) -> Result<IpDetails, IpError>
    {
        if let Some(details) = self.cache.get(&ip.to_string())
        {
            Ok(details.clone())
        }
        else
        {
            match self.native_client.lookup(ip).await
            {
                Ok(details) => {

                    self.cache.insert(ip.to_string(), details.clone());
                    Ok(details)

                }
                Err(err) => { Err(err) }
            }
        }
    }
}