use std::net::{SocketAddr, TcpListener, TcpStream};
use std::str::FromStr;
use std::sync::{Arc};
use crate::proxy_model::{Proxy};
use std::io::{copy, Read, Write, Result, Error};
use std::process::exit;
use std::thread;
use std::time::Duration;


const SOCKS_VERSION: u8 = 0x05;
const AUTHENTICATION_VERSION: u8 = 0x01;

#[derive(Debug, Clone)]
pub struct ForwardProxy {
    addr: SocketAddr,
    proxy: Proxy,
    server: Arc<TcpListener>,
}

impl ForwardProxy {
    pub fn new_with_proxy(port: u16, proxy: Proxy) -> Result<ForwardProxy> {
        let addr = SocketAddr::from(([127, 0, 0, 1], port));
        if Self::check_port(addr) {
            return Err(
                Error::new(
                    std::io::ErrorKind::Other,
                    "Port is not available",
                )
            );
        }
        let server = TcpListener::bind(&addr)?;
        let mut sv = ForwardProxy {
            addr,
            proxy,
            server: Arc::new(server),
        };
        match sv.check_proxy() {
            Ok(_) => {
                Ok(sv)
            }
            Err(_) => {
                return Err(
                    Error::new(
                        std::io::ErrorKind::Other,
                        "Proxy is not working",
                    )
                );
            }
        }
    }

    fn remote(proxy: Arc<Proxy>) -> Result<TcpStream> {
        // create a connection
        let proxy_url = format!("{}:{}", proxy.uri, proxy.port);
        let mut remote_stream = TcpStream::connect(proxy_url).map_err(|e| {
            Error::new(std::io::ErrorKind::Other, format!("Failed to connect to proxy: {}", e))
        })?;

        // greeting header
        remote_stream.write(&[
            SOCKS_VERSION, // SOCKS version
            0x01, // Number of authentication methods
            0x02, // Username/password authentication
        ])?;

        // Receive the servers reply
        let mut buffer: [u8; 2] = [0; 2];
        remote_stream.read(&mut buffer)?;

        // Check the SOCKS version
        if buffer[0] != SOCKS_VERSION {
            return Err(
                Error::new(
                    std::io::ErrorKind::Other,
                    format!(
                        "Server does not support socks version: {}",
                        SOCKS_VERSION
                    ),
                )
            );
        }

        // Check the authentication method
        if buffer[1] != 0x02 {
            return Err(
                Error::new(
                    std::io::ErrorKind::Other,
                    "Server does not support username/password authentication",
                )
            );
        }
        if proxy.auth.is_none() {
            return Err(
                Error::new(
                    std::io::ErrorKind::Other,
                    "Proxy requires authentication",
                )
            );
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
            return Err(
                Error::new(
                    std::io::ErrorKind::Other,
                    format!(
                        "Unsupported username/password authentication version: {}",
                        buffer[0]
                    ),
                )
            );
        }

        // Check the username/password authentication status
        if buffer[1] != 0x00 {
            return Err(
                Error::new(
                    std::io::ErrorKind::Other,
                    "Username/password authentication failed",
                )
            );
        }

        // Return the stream
        Ok(remote_stream)
    }

    fn client(mut local_stream: TcpStream, proxy: Arc<Proxy>) -> Result<()> {
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
            return Err(Error::new(std::io::ErrorKind::Other, "Method not supported"));
        }

        // we choose no authentication
        local_stream.write(&[SOCKS_VERSION, 0x00])?;

        // create a TcpStream to the remote server
        let mut remote_stream: TcpStream = Self::remote(proxy)?;

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

    fn check_port(addr: SocketAddr) -> bool {
        TcpStream::connect_timeout(&addr, Duration::from_secs(5)).is_ok()
    }

    pub fn check_proxy(&mut self) -> Result<bool> {
        let start = std::time::Instant::now();
        let proxy = Arc::new(self.proxy.clone());
        let mut remote = Self::remote(proxy)?;
        let dest = "httpbin.org:80";
        remote.write(&[
            SOCKS_VERSION, // SOCKS version
            0x01, // Connect
            0x00, // Reserved
            0x03, // Domain name
            dest.len() as u8, // Domain name length
        ])?;
        remote.write(dest.as_bytes())?;
        remote.write(&[0x00, 0x50])?;
        let mut buffer: [u8; 10] = [0; 10];
        remote.read(&mut buffer)?;
        let latency = start.elapsed();
        self.proxy.latency = latency;
        self.proxy.is_working = true;
        Ok(true)
    }

    pub fn get_proxy(&self) -> Proxy {
        self.proxy.clone()
    }

    pub fn get_addr(&self) -> SocketAddr {
        self.addr.clone()
    }

    pub fn start(&self) -> Result<()> {
        if !self.proxy.is_working {
            println!("Proxy is not working");
            return Err(
                Error::new(
                    std::io::ErrorKind::Other,
                    "Proxy is not working",
                )
            );
        }
        println!("Listening on: socks5://{}", self.addr);
        let proxy = Arc::new(self.proxy.clone());

        loop {
            match self.server.accept() {
                Ok((stream, _)) => {
                    let proxy = proxy.clone();
                    thread::spawn(move || {
                        Self::client(stream, proxy).unwrap();
                    });
                }
                Err(e) => {
                    println!("Failed to accept connection: {}", e);
                }
            }
        }
    }

    pub fn stop(&self) {
        println!("Action Stopping server");
        exit(0)
    }
}


impl TryFrom<(u16, Proxy)> for ForwardProxy {
    type Error = Error;

    fn try_from(value: (u16, Proxy)) -> Result<Self> {
        ForwardProxy::new_with_proxy(value.0, value.1)
    }
}

impl TryFrom<(u16, String)> for ForwardProxy {
    type Error = Error;

    fn try_from((port, proxy_str): (u16, String)) -> Result<Self> {
        let proxy = Proxy::from_str(&proxy_str).map_err(|e| Error::new(std::io::ErrorKind::Other, e))?;
        return ForwardProxy::try_from((port, proxy));
    }
}