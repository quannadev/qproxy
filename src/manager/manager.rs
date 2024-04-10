#![allow(unused)]
use crate::errors::ProxyError;
use crate::{Proxy, ProxyServer};
use log::{error, info, warn};
use rayon::iter::{ParallelBridge, ParallelIterator};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::process::Command;
use std::str::FromStr;
use std::sync::atomic::AtomicI16;
use std::sync::mpsc::channel;
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use tokio::fs::File;
use tokio::io::{AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::sync::Mutex;
use tokio::time;

#[derive(Debug)]
pub struct ProxyManager {
    proxies: Arc<Mutex<Vec<Proxy>>>,
    servers: Arc<Mutex<Vec<ProxyServer>>>,
    port_seq: AtomicI16,
    rotate_interval: i64, // in seconds
}

impl ProxyManager {
    pub async fn new(proxies_path: &str, start_port: i16, rotate_interval: i64) -> Self {
        let proxies = ProxyManager::load_proxies(proxies_path.to_string())
            .await
            .unwrap_or_default();
        ProxyManager {
            proxies: Arc::new(Mutex::new(proxies)),
            servers: Arc::new(Mutex::new(Vec::new())),
            port_seq: AtomicI16::new(start_port),
            rotate_interval,
        }
    }

    async fn load_proxies(proxies_path: String) -> Result<Vec<Proxy>, ProxyError> {
        let mut proxies = Vec::<Proxy>::new();

        let file = File::open(proxies_path)
            .await
            .map_err(|e| ProxyError::LoadProxiesError(e.to_string()))?;
        let mut content = String::new();
        BufReader::new(file)
            .read_to_string(&mut content)
            .await
            .map_err(|e| ProxyError::LoadProxiesError(e.to_string()))?;
        let checked_proxies = "checked_proxies.txt";
        if let Ok(file) = File::open(checked_proxies).await {
            let mut content = String::new();
            BufReader::new(file)
                .read_to_string(&mut content)
                .await
                .map_err(|e| ProxyError::LoadProxiesError(e.to_string()))?;
            proxies = content
                .lines()
                .map(|line| Proxy::from_str(line))
                .filter_map(Result::ok)
                .collect();
        }

        let (sender, receiver) = channel::<Proxy>();

        // Check each proxy in parallel
        content
            .lines()
            .par_bridge()
            .for_each_with(sender, |s, line| {
                Proxy::from_str(line)
                    .map_err(|e| {
                        error!("Failed to parse proxy: {}", e);
                    })
                    .and_then(|proxy| {
                        if proxies.contains(&proxy) {
                            return Ok(());
                        }
                        ProxyServer::check_proxy(proxy)
                            .map_err(|e| {
                                error!("Failed to check proxy: {}", e);
                            })
                            .and_then(|proxy| {
                                info!("proxy: {} live", proxy.to_string());
                                s.send(proxy).map_err(|e| {
                                    error!("Failed to send proxy: {}", e);
                                })
                            })
                    });
            });

        while let Ok(proxy) = receiver.recv() {
            proxies.push(proxy);
        }

        // Sort proxies by latency
        proxies.sort_by(|a, b| a.latency.cmp(&b.latency));

        info!("Loaded {} live proxies", proxies.len());

        // save to checked_proxies.txt
        let mut file = File::create("checked_proxies.txt").await?;
        for proxy in proxies.iter() {
            file.write_all(format!("{}\n", proxy.to_string()).as_bytes())
                .await?;
        }

        Ok(proxies)
    }

    pub async fn proxies(&self) -> Vec<Proxy> {
        self.proxies.lock().await.clone().to_vec()
    }

    pub async fn servers(&self) -> Vec<ProxyServer> {
        self.servers.lock().await.clone().to_vec()
    }

    pub async fn create_server(&self, proxy: Proxy) -> Result<SocketAddr, ProxyError> {
        let last_port = self.port_seq.load(std::sync::atomic::Ordering::SeqCst);
        let server = ProxyServer::new_with_proxy(last_port, proxy.clone())?;
        let server_addr = server.get_addr();
        let mut servers = self.servers.lock().await;
        servers.push(server.clone());

        self.port_seq
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        tokio::spawn(async move { server.start() });
        Ok(server_addr)
    }

    async fn get_last_proxy(&self) -> Option<Proxy> {
        let mut proxies = self.proxies().await;
        if proxies.is_empty() {
            return None;
        }
        let proxy = self.proxies.lock().await.remove(0);
        self.proxies.lock().await.push(proxy.clone());
        Some(proxy)
    }

    async fn get_server_by_proxy(&self, proxy: &Proxy) -> Option<ProxyServer> {
        let servers = self.servers().await;
        for x in servers {
            if x.get_proxy().unwrap().ip == proxy.ip {
                return Some(x);
            }
        }
        None
    }

    pub async fn stop_server(&self, proxy: &Proxy) -> Result<(), ProxyError> {
        let mut servers = self.servers().await;
        let server = self.get_server_by_proxy(proxy).await;
        match server {
            Some(server) => {
                let addr = server.get_addr();
                let mut servers = self.servers.lock().await;
                servers.retain(|x| x.get_addr() != addr);
                Ok(())
            }
            None => Err(ProxyError::ServerError("Server not found".to_string())),
        }
    }

    async fn rotate_proxy(&self) -> Result<(), ProxyError> {
        info!("Rotating proxies");
        loop {
            if self.rotate_interval == 0 || self.proxies().await.is_empty() {
                error!("No proxies available for rotation");
                break;
            }
            time::sleep(Duration::from_secs(self.rotate_interval as u64)).await;

            let servers = self.servers.lock().await;
            info!("Check and rotating proxies for {} servers", servers.len());

            for server in servers.iter() {
                let old_proxy = server.get_proxy().unwrap();
                let duration = server.get_duration().as_secs();
                info!(
                    "Checking proxy: {} | server time {}s",
                    old_proxy.to_string(),
                    duration
                );

                if duration >= self.rotate_interval as u64 {
                    let new_proxy = match self.get_last_proxy().await {
                        Some(p) => p,
                        None => {
                            error!("No proxies available for rotation");
                            continue;
                        }
                    };
                    if new_proxy.eq(&old_proxy) {
                        warn!("New proxy is the same as the old proxy");
                        continue;
                    }
                    server.set_proxy(new_proxy.clone());
                } else {
                    info!(
                        "Proxy {} is still fresh {} seconds",
                        old_proxy.to_string(),
                        duration
                    );
                }
            }
            drop(servers)
        }
        info!("Rotating proxies finished");
        Ok(())
    }

    pub async fn start(&self) -> Result<(), ProxyError> {
        if self.proxies().await.is_empty() {
            return Err(ProxyError::ProxyNotSet);
        }
        if self.servers().await.is_empty() {
            let last_proxy = self.get_last_proxy().await.expect("No proxies available");
            let addr = self.create_server(last_proxy).await?;
            info!("Started proxy server on: {}", addr);
        }
        //Self::test_browser();
        self.rotate_proxy().await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::time;

    #[tokio::test]
    async fn test_load_proxies() {
        let proxies_path = "proxies.txt".to_string();
        let proxies = ProxyManager::load_proxies(proxies_path).await.unwrap();
        assert_eq!(proxies.len(), 100);
    }
}
