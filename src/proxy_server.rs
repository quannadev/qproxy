use std::net::{SocketAddr, TcpListener, TcpStream};
use std::str::FromStr;
use std::sync::Arc;
use crate::proxy_model::{Proxy};
use std::io::{copy, Read, Write, Result, Error};
use std::thread;


const SOCKS_VERSION: u8 = 0x05;
const AUTHENTICATION_VERSION: u8 = 0x01;

#[derive(Debug, Clone)]
pub struct ForwardProxy {
    port: u16,
    addr: SocketAddr,
    proxy: Proxy,
}

impl ForwardProxy {
    pub async fn new(port: u16, proxy_str: String) -> Result<ForwardProxy> {
        let addr = SocketAddr::from(([127, 0, 0, 1], port));
        let proxy = Proxy::from_str(&proxy_str).map_err(|e| Error::new(std::io::ErrorKind::Other, e))?;
        Ok(ForwardProxy {
            port,
            addr,
            proxy,
        })
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

    pub fn start(&self) -> Result<()> {
        let server = TcpListener::bind(&self.addr)?;
        println!("Listening on: socks5://{}", self.addr);
        let proxy = Arc::new(self.proxy.clone());
        for stream in server.incoming() {
            match stream {
                Ok(stream) => {
                    let proxy = proxy.clone();
                    thread::spawn(move || {
                        Self::client(stream, proxy).unwrap();
                    });
                }
                Err(e) => {
                    eprintln!("Error: {}", e);
                }
            }
        }

        Ok(())
    }
}

