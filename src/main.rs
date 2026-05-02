use std::net::{self, TcpListener, TcpStream};
use std::io::prelude::*;

fn send_web(stream: &mut TcpStream) -> std::io::Result<()> {
    println!("got connection");
    let mut buffer = [0; 1024];
    let n = stream.read(&mut buffer)?;
    let request = String::from_utf8_lossy(&buffer[..n]);
    let request_line = request.lines().next().unwrap_or("");
    if request_line.contains("GET / HTTP/1.1") {
        stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=UTF-8\r\nContent-Length: 13\r\nConnection: close\r\n\r\nHello, World!");
    
    } else {
        stream.write_all(b"HTTP/1.1 404");
    }
    Ok(())
}

fn main() -> std::io::Result<()>{
    println!("rusttp");
    let listener = TcpListener::bind("127.0.0.1:8080")?;
    for stream in listener.incoming() {
        if let Ok(mut s) = stream {
            let _ = send_web(&mut s);
        }
    }
    Ok(())
}
