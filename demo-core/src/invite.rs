//! `logos-pair:v1:<runtime id>:<root digest>:<secret>@<host>:<port>`, as
//! logos-peering prints it. Only the parts the app shows are read here.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invite {
    pub runtime_id: String,
    pub host: String,
    pub port: u16,
}

impl Invite {
    pub fn parse(text: &str) -> Result<Invite, String> {
        let bad = || "not a Logos invite (logos-pair:v1:...)".to_string();
        let rest = text.trim().strip_prefix("logos-pair:v1:").ok_or_else(bad)?;
        let (fields, address) = rest.rsplit_once('@').ok_or_else(bad)?;
        let runtime_id = fields.split(':').next().filter(|id| !id.is_empty()).ok_or_else(bad)?;
        let (host, port) = address.rsplit_once(':').ok_or_else(bad)?;
        let host = host.trim_start_matches('[').trim_end_matches(']');
        Ok(Invite {
            runtime_id: runtime_id.to_string(),
            host: host.to_string(),
            port: port.parse().map_err(|_| bad())?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_peer_and_its_address() {
        let invite = Invite::parse("logos-pair:v1:6f1c2e3a-0000-4000-8000-000000000001:AbC:s3cr3t@192.168.1.5:7443\n")
            .unwrap();
        assert_eq!(invite.runtime_id, "6f1c2e3a-0000-4000-8000-000000000001");
        assert_eq!(invite.host, "192.168.1.5");
        assert_eq!(invite.port, 7443);
        assert!(Invite::parse("https://example.com").is_err());
        assert!(Invite::parse("logos-pair:v1:id:d:s@host").is_err());
    }
}
