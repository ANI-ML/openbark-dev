//! Advertising the hub on the local network (ADR-0029 section 5): mDNS /
//! DNS-SD `_openbark-hub._tcp` with its port and the first 16 hex
//! characters of its certificate fingerprint. Only while hub mode is on.
//! Devices still check the join code and the certificate: an advertisement
//! is only a hint (docs/network.md L2).

use crate::HubError;

pub struct Advertiser {
    daemon: mdns_sd::ServiceDaemon,
    fullname: String,
}

impl Advertiser {
    pub fn start(port: u16, fingerprint: &[u8; 32]) -> Result<Self, HubError> {
        let err =
            |e: mdns_sd::Error| HubError::Io(format!("couldn't advertise the hub on the local network: {e}"));
        let daemon = mdns_sd::ServiceDaemon::new().map_err(err)?;
        let prefix = hex::encode(&fingerprint[..8]);
        let instance = format!("OpenBark hub {}", &prefix[..6]);
        let host = format!("openbark-hub-{}.local.", &prefix[..8]);
        let props = [("fp", prefix.as_str()), ("v", "1")];
        let info =
            mdns_sd::ServiceInfo::new(os_sync::hub::SERVICE_TYPE, &instance, &host, (), port, &props[..])
                .map_err(err)?
                .enable_addr_auto();
        let fullname = info.get_fullname().to_string();
        daemon.register(info).map_err(err)?;
        Ok(Self { daemon, fullname })
    }
}

impl Drop for Advertiser {
    fn drop(&mut self) {
        let _ = self.daemon.unregister(&self.fullname);
        let _ = self.daemon.shutdown();
    }
}
