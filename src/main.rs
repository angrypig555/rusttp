use std::fs;
use std::net::{self, TcpListener, TcpStream};
use std::io::prelude::*;
use std::path::Path;
use serde::Deserialize;
use shellexpand;
use std::process;
use std::path::Component;
use mime_guess;
use std::thread;
use std::time::Duration;
use threadpool::ThreadPool;
use threadpool::Builder;
use std::sync::Arc;
use std::path::PathBuf;

#[derive(Deserialize)]
struct Config {
    port: u16,
    listen_ip: String,
    web_dir: String,
    multithreading: bool,
    workers: u16,
}

fn send_web(stream: &mut TcpStream, web_dir: &Path) -> std::io::Result<()> {
    let mut buffer = [0; 1024];
    let n = stream.read(&mut buffer)?;
    let request = String::from_utf8_lossy(&buffer[..n]);
    let request_line = request.lines().next().unwrap_or("");
    let parts: Vec<&str> = request_line.split_whitespace().collect();
    if parts.len() < 2 {
        return Ok(());
    }
    let mut requested_path_raw = parts[1];
    if requested_path_raw == "/" {
        requested_path_raw = "index.html";
    }
    let filename = if requested_path_raw == "/" { "index.html" } else { requested_path_raw.trim_start_matches('/') };
    let requested_path = Path::new(filename);
    if requested_path.components().any(|c| matches!(c, Component::ParentDir)) {
        stream.write_all(b"HTTP/1.1 403 FORBIDDEN\r\nServer: rusttp/1.0\r\n\r\n")?;
        return Ok(());
    }
    let full_path = web_dir.join(requested_path);
    if full_path.exists() {
        let content = fs::read(&full_path)?;
        let length = content.len();
        let mime_type = mime_guess::from_path(&full_path).first_or_octet_stream().to_string();
        let response = format!(
            "HTTP/1.1 200 OK\r\nServer: rusttp/1.0\r\nContent-Type: {}; charset=UTF-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            mime_type,
            length,
        );
        stream.write_all(response.as_bytes())?;
        stream.write_all(&content)?;
    
    } else {
        stream.write_all(b"HTTP/1.1 404 NOT FOUND\r\nServer: rusttp/1.0\r\nContent-Length: 0\r\n\r\n")?;
    }
    Ok(())
}

fn main() -> std::io::Result<()>{
    let placeholder_webpage = r#"
    <!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>rusttp test page</title>
    <link rel="stylesheet" type="text/css" href="style.css">
</head>
<body>
    <h1>rusttp test page</h1>
    <hr>
    <p>if you see this, rusttp is working but you do not have any websites in the correct folder</p>
</body>
</html>"#;
    let placeholder_css = r#"
    html {
    text-align: center;
}"#;
    let config_file_raw = shellexpand::tilde("~/.config/rusttp/config.toml");
    let config_file = Path::new(config_file_raw.as_ref());
    let config_folder_raw = shellexpand::tilde("~/.config/rusttp");
    let config_folder = Path::new(config_folder_raw.as_ref());
    if !config_folder.is_dir() {
        println!("config folder not found, creating config folder at ~/.config/rusttp");
        fs::create_dir_all(config_folder)?;
    }
    if !config_file.exists() {
        println!("config file does not exist, creating config file at ~/.config/rusttp/config.toml");
        let default_conf = "port = 8080\nlisten_ip = \"127.0.0.1\"\nweb_dir = \"~/www\"\nmultithreading = true\nworkers = 10\n#When not using multithreading, the workers value will be disregarded";
        fs::write(config_file, default_conf);
        println!("default configuration of \n{} has been applied", default_conf);
    }
    let conf_contents = fs::read_to_string(config_file)
        .expect("Unable to read config file");
    let conf: Config = toml::from_str(&conf_contents).expect("Configuration file invalid, have you tried deleting the config file?");
    let workers = conf.workers;
    let use_multithreading = conf.multithreading;
    println!("Loaded config\nListening on IP {}\nPort {}\nWeb directory {}\nMultithreading {}\nWorkers {}", conf.listen_ip, conf.port, conf.web_dir, conf.multithreading, conf.workers);
    let web_dir_raw = shellexpand::tilde(&conf.web_dir);
    let web_dir = Arc::new(PathBuf::from(web_dir_raw.into_owned()));
    if !web_dir.exists() {
        println!("web directory {} does not exist, creating folder", conf.web_dir);
        fs::create_dir_all(&*web_dir);
    }
    let index = web_dir.join("index.html");
    if !index.exists() {
        println!("error: could not find index.html at {}", index.display());
        fs::write(&index, placeholder_webpage)?;
        fs::write(web_dir.join("style.css"), placeholder_css)?;
    }
    
    println!("rusttp");
    let pool = if use_multithreading {
        println!("{} workers started", workers);
        Some(Builder::new().num_threads(workers.into()).thread_name("worker".into()).build())
    } else {
        println!("multithreading disabled");
        None
    };
    let combinedip = format!("{}:{}", conf.listen_ip, conf.port);
    let listener = TcpListener::bind(combinedip)?;
    for stream in listener.incoming() {
        if let Ok(mut s) = stream {
            let thread_web_dir = Arc::clone(&web_dir);
            match &pool {
                Some(p) => p.execute(move || {
                    println!("({:?}) got connection", thread::current().id());
                    let _ = send_web(&mut s, &thread_web_dir);
                }),
                None => {
                    println!("got connection");
                    let _ = send_web(&mut s, &thread_web_dir);
                }
            }
        }
    }
    Ok(())
}
