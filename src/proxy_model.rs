use std::str::FromStr;

#[derive(Debug, Clone)]
pub struct ProxyAuth {
    pub user: String,
    pub pass: String,
}

#[derive(Debug, Clone)]
pub struct Proxy {
    pub uri: String,
    pub port: u16,
    pub auth: Option<ProxyAuth>,
}

impl FromStr for Proxy {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let parts: Vec<&str> = s.split(":").collect();
        if parts.len() < 3 {
            return Err("Invalid proxy string".to_string());
        }
        let uri = parts[0].parse().expect("Invalid proxy URI");
        let port = parts[1].parse().expect("Invalid proxy port");
        if parts.len() == 2 {
            return Ok(Proxy {
                uri,
                port,
                auth: None,
            });
        }
        let user = parts[2].parse().expect("Invalid proxy user");
        let pass = parts[3].parse().expect("Invalid proxy pass");
        Ok(Proxy {
            uri,
            port,
            auth: Some(ProxyAuth { user, pass }),
        })
    }
}