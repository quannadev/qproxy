
#[derive(Debug, thiserror::Error)]
pub enum ProxyError {
    #[error("Failed to connect to proxy: {0}")]
    ForwardProxyError(#[from] std::io::Error),
    #[error("Proxy not set or empty")]
    ProxyNotSet,
    #[error("List proxies require >= 2 : {0}")]
    ProxiesTooSmall(u64),
    #[error("Failed to load proxies: {0}")]
    LoadProxiesError(String),
    #[error("Error server: {0}")]
    ServerError(String),
    #[error("Server not found {0}")]
    ServerNotFound(String),
}