use std::fmt::Display;
use std::str::FromStr;
use std::time::Duration;

#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub struct ProxyAuth {
    pub user: String,
    pub pass: String,
}

#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub struct Proxy {
    pub ip: String,
    pub port: u16,
    pub auth: Option<ProxyAuth>,
    pub is_working: bool,
    pub latency: Duration,
    pub used: bool,
}

impl FromStr for Proxy {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let parts: Vec<&str> = s.split(":").collect();
        if parts.len() < 3 {
            return Err("Invalid proxy string".to_string());
        }
        let uri = parts[0].parse().expect("Invalid proxy IP");
        let port = parts[1].parse().expect("Invalid proxy port");
        if parts.len() == 2 {
            return Ok(Proxy {
                ip: uri,
                port,
                auth: None,
                is_working: false,
                latency: Duration::from_secs(0),
                used: true,
            });
        }
        let user = parts[2].parse().expect("Invalid proxy user");
        let pass = parts[3].parse().expect("Invalid proxy pass");
        Ok(Proxy {
            ip: uri,
            port,
            auth: Some(ProxyAuth { user, pass }),
            is_working: false,
            latency: Duration::from_secs(0),
            used: true,
        })
    }
}

impl Display for Proxy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let str = match &self.auth {
            Some(auth) => format!("{}:{}:{}:{}", self.ip, self.port, auth.user, auth.pass),
            None => format!("{}:{}", self.ip, self.port),
        };
        write!(f, "{}", str)
    }
}