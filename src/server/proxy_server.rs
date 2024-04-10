use crate::Proxy;
use log::{error, info};
use std::io::{copy, Error, Read, Result, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use std::thread;

const SOCKS_VERSION: u8 = 0x05;
const AUTHENTICATION_VERSION: u8 = 0x01;

#[derive(Debug, Clone)]
pub struct ProxyServer {
    addr: SocketAddr,
    proxy: Arc<Mutex<Proxy>>,
    should_stop: Arc<Mutex<bool>>,
    started_at: Arc<Mutex<std::time::Instant>>,
}

impl ProxyServer {
    pub fn new_with_proxy(port: i16, proxy: Proxy) -> Result<ProxyServer> {
        let addr = format!("127.0.0.1:{port}").parse().unwrap();
        Ok(ProxyServer {
            addr,
            proxy: Arc::new(Mutex::new(proxy)),
            should_stop: Arc::new(Mutex::new(false)),
            started_at: Arc::new(Mutex::new(std::time::Instant::now())),
        })
    }

    fn remote(proxy: Proxy) -> Result<TcpStream> {
        // create a connection
        let proxy_url = format!("{}:{}", proxy.ip, proxy.port);
        let mut remote_stream = TcpStream::connect(proxy_url).map_err(|e| {
            Error::new(
                std::io::ErrorKind::Other,
                format!("Failed to connect to proxy: {}", e),
            )
        })?;

        // greeting header
        remote_stream.write(&[
            SOCKS_VERSION, // SOCKS version
            0x01,          // Number of authentication methods
            0x02,          // Username/password authentication
        ])?;

        // Receive the servers reply
        let mut buffer: [u8; 2] = [0; 2];
        remote_stream.read(&mut buffer)?;

        // Check the SOCKS version
        if buffer[0] != SOCKS_VERSION {
            return Err(Error::new(
                std::io::ErrorKind::Other,
                format!("Server does not support socks version: {}", SOCKS_VERSION),
            ));
        }

        // Check the authentication method
        if buffer[1] != 0x02 {
            return Err(Error::new(
                std::io::ErrorKind::Other,
                "Server does not support username/password authentication",
            ));
        }
        if proxy.auth.is_none() {
            return Err(Error::new(
                std::io::ErrorKind::Other,
                "Proxy requires authentication",
            ));
        }

        let proxy_auth = proxy.auth.as_ref().unwrap();

        // Create a username/password negotiation request
        let username: &str = proxy_auth.user.as_str();
        let password: &str = proxy_auth.pass.as_str();

        let mut auth_request = vec![
            AUTHENTICATION_VERSION, // Username/password authentication version
        ];

        auth_request.push(username.len() as u8); // Username length
        auth_request.extend_from_slice(username.as_bytes());
        auth_request.push(password.len() as u8); // Password length
        auth_request.extend_from_slice(password.as_bytes());

        // Send the username/password negotiation request
        remote_stream.write(&auth_request)?;

        // Receive the username/password negotiation reply/welcome message
        let mut buffer: [u8; 2] = [0; 2];
        remote_stream.read(&mut buffer)?;

        // Check the username/password authentication version
        if buffer[0] != AUTHENTICATION_VERSION {
            return Err(Error::new(
                std::io::ErrorKind::Other,
                format!(
                    "Unsupported username/password authentication version: {}",
                    buffer[0]
                ),
            ));
        }

        // Check the username/password authentication status
        if buffer[1] != 0x00 {
            return Err(Error::new(
                std::io::ErrorKind::Other,
                "Username/password authentication failed",
            ));
        }

        // Return the stream
        Ok(remote_stream)
    }

    fn client(mut local_stream: TcpStream, mut remote_stream: TcpStream) -> Result<()> {
        // greeting header
        let mut buffer: [u8; 2] = [0; 2];
        local_stream.read(&mut buffer[..])?;
        let _version = buffer[0]; // should be the same as SOCKS_VERSION
        let number_of_methods = buffer[1];

        // authentication methods
        let mut methods: Vec<u8> = vec![];
        for _ in 0..number_of_methods {
            let mut next_method: [u8; 1] = [0; 1];
            local_stream.read(&mut next_method[..])?;
            methods.push(next_method[0]);
        }

        // only accept no authentication
        if !methods.contains(&0x00) {
            // no acceptable methods were offered
            local_stream.write(&[SOCKS_VERSION, 0xFF])?;
            return Err(Error::new(
                std::io::ErrorKind::Other,
                "Method not supported",
            ));
        }

        // we choose no authentication
        local_stream.write(&[SOCKS_VERSION, 0x00])?;
        // clone our streams
        let mut incoming_local = local_stream.try_clone()?;
        let mut incoming_remote = remote_stream.try_clone()?;

        // copy the data from one to the other
        let handle_outgoing = thread::spawn(move || -> Result<()> {
            copy(&mut local_stream, &mut remote_stream)?;
            Ok(())
        });

        let handle_incoming = thread::spawn(move || -> Result<()> {
            copy(&mut incoming_remote, &mut incoming_local)?;
            Ok(())
        });

        _ = handle_outgoing.join();
        _ = handle_incoming.join();

        // The End.
        Ok(())
    }

    pub fn check_proxy(proxy: Proxy) -> Result<Proxy> {
        let start = std::time::Instant::now();
        let mut remote = Self::remote(proxy.clone())?;
        let dest = "httpbin.org:80";
        remote.write(&[
            SOCKS_VERSION,    // SOCKS version
            0x01,             // Connect
            0x00,             // Reserved
            0x03,             // Domain name
            dest.len() as u8, // Domain name length
        ])?;
        remote.write(dest.as_bytes())?;
        remote.write(&[0x00, 0x50])?;
        let mut buffer: [u8; 10] = [0; 10];
        remote.read(&mut buffer)?;
        let latency = start.elapsed();
        let mut new_proxy = proxy.clone();
        new_proxy.latency = latency;
        new_proxy.is_working = true;
        Ok(new_proxy)
    }

    pub fn get_proxy(&self) -> Option<Proxy> {
        self.proxy.lock().map_or(None, |p| Some(p.clone()))
    }

    pub fn get_addr(&self) -> SocketAddr {
        self.addr.clone()
    }

    pub fn get_duration(&self) -> std::time::Duration {
        self.started_at.lock().unwrap().elapsed()
    }

    pub fn set_proxy(&self, new_proxy: Proxy) -> Result<()> {
        match ProxyServer::check_proxy(new_proxy.clone()) {
            Ok(p) => {
                let mut proxy = self.proxy.lock().unwrap();
                *proxy = p;
                let mut started_at = self.started_at.lock().unwrap();
                *started_at = std::time::Instant::now();
                info!("Proxy changed to: {}", proxy);
                Ok(())
            }
            Err(e) => {
                error!("Failed to check proxy: {:?}", e);
                return Err(e);
            }
        }
    }

    pub fn start(&self) -> Result<()> {
        info!(
            "Starting proxy server on: {} | Proxy {}",
            self.addr,
            self.proxy.lock().unwrap().ip
        );
        let server = TcpListener::bind(self.addr)?;
        for stream in server.incoming() {
            if *self.should_stop.lock().unwrap() {
                break;
            }
            match stream {
                Ok(stream) => match self.proxy.lock() {
                    Ok(proxy) => {
                        let remote_stream: TcpStream = Self::remote(proxy.clone())?;
                        thread::spawn(move || match Self::client(stream, remote_stream) {
                            Ok(_) => {}
                            Err(e) => {
                                error!("Failed to handle client: {:?}", e);
                            }
                        });
                    }
                    Err(e) => {
                        error!("Failed to get proxy: {:?}", e);
                        continue;
                    }
                },
                Err(e) => {
                    error!("Failed to accept connection: {:?}", e);
                    return Err(e);
                }
            }
        }

        info!(
            "Proxy server stopped on: {} duration: {}",
            self.addr,
            self.started_at.lock().unwrap().elapsed().as_secs()
        );
        Ok(())
    }

    pub fn stop(&self) {
        *self.should_stop.lock().unwrap() = true;
        info!("Stopping proxy server on: {}", self.addr);
        let stream = TcpStream::connect(self.addr).unwrap();
        drop(stream);
    }
}

impl TryFrom<(i16, Proxy)> for ProxyServer {
    type Error = Error;

    fn try_from(value: (i16, Proxy)) -> Result<Self> {
        ProxyServer::new_with_proxy(value.0, value.1)
    }
}

impl TryFrom<(i16, String)> for ProxyServer {
    type Error = Error;

    fn try_from((port, proxy_str): (i16, String)) -> Result<Self> {
        let proxy =
            Proxy::from_str(&proxy_str).map_err(|e| Error::new(std::io::ErrorKind::Other, e))?;
        return ProxyServer::try_from((port, proxy));
    }
}
