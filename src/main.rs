use std::fs::{self, create_dir_all};
use std::iter::Once;
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
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use chrono::Local;
use std::fs::OpenOptions;

#[derive(Deserialize)]
struct Config {
    port: u16,
    listen_ip: String,
    web_dir: String,
    multithreading: bool,
    workers: u16,
    logging: bool,
    log_dir: String,
}

static LOG_PATH: OnceLock<PathBuf> = OnceLock::new();
static LOGGING_ENABLED: AtomicBool = AtomicBool::new(false);

fn log_init(raw_path: &str) {
    let log_folder = shellexpand::tilde(raw_path).into_owned();
    let path = PathBuf::from(log_folder);

    let log_file = path.join("rusttp.log");
    let curr_time = Local::now();
    let log_header = format!("rusttp v1.0 - logs - {}\n", curr_time);
    if !path.exists() {
        create_dir_all(path);
    }
    if !log_file.exists() {
        fs::write(&log_file, &log_header);
    }
    if log_file.exists() {
        fs::remove_file(&log_file);
        fs::write(&log_file, &log_header);
    }
    LOG_PATH.set(log_file);
}

fn log(msg: &str) {
    if !LOGGING_ENABLED.load(Ordering::Relaxed) {
        return;
    }
    let curr_time = Local::now();
    let message = format!("{:?} - {}: {}\n", thread::current().id(), curr_time, msg);
    if let Some(path) = LOG_PATH.get() {
        let file = OpenOptions::new().create(true).append(true).open(path);
        if let Ok(mut file) = file {
            let _ = file.write_all(message.as_bytes());
        }
    }
    

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
        log(&format!("returned 403 error, tried to access {}", requested_path.display()));
        return Ok(());
    }
    let full_path = web_dir.join(requested_path);
    if full_path.exists() {
        let content = fs::read(&full_path)?;
        log(&format!("requested path exists, serving {} to client", &full_path.display()));
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
        log(&format!("requested path does not exist, client requested {} but was not found", &full_path.display()));
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
        let default_conf = "port = 8080\nlisten_ip = \"127.0.0.1\"\nweb_dir = \"~/www\"\nmultithreading = true\nworkers = 10\n#When not using multithreading, the workers value will be disregarded\nlogging = true\n log_dir = \"~/.cache/rusttp/\"\n# Log directory will be disregarded if logging is disabled";
        fs::write(config_file, default_conf);
        println!("default configuration of \n{} has been applied", default_conf);
    }
    let conf_contents = fs::read_to_string(config_file)
        .expect("Unable to read config file");
    let conf: Config = toml::from_str(&conf_contents).expect("Configuration file invalid, have you tried deleting the config file?");
    let workers = conf.workers;
    let use_multithreading = conf.multithreading;
    let config_msg = format!("Loaded config\nListening on IP {}\nPort {}\nWeb directory {}\nMultithreading {}\nWorkers {}\nLogging {}\nLog Directory {}", conf.listen_ip, conf.port, conf.web_dir, conf.multithreading, conf.workers, conf.logging, conf.log_dir);
    println!("{}", &config_msg);
    if conf.logging == true {
        LOGGING_ENABLED.store(true, Ordering::Relaxed);
        log_init(&conf.log_dir);
    }
    log(&config_msg);
    let web_dir_raw = shellexpand::tilde(&conf.web_dir);
    let web_dir = Arc::new(PathBuf::from(web_dir_raw.into_owned()));
    if !web_dir.exists() {
        println!("web directory {} does not exist, creating folder", conf.web_dir);
        log(&format!("web directory {} does not exist, creating folder", conf.web_dir));
        fs::create_dir_all(&*web_dir);
    }
    let index = web_dir.join("index.html");
    if !index.exists() {
        println!("error: could not find index.html at {}", index.display());
        log(&format!("error: could not find index.html at {}", index.display()));
        fs::write(&index, placeholder_webpage)?;
        fs::write(web_dir.join("style.css"), placeholder_css)?;
        log("using placeholder page");
    }
    
    println!("rusttp");
    let pool = if use_multithreading {
        println!("{} workers started", workers);
        log(&format!("{} workers started", workers));
        Some(Builder::new().num_threads(workers.into()).thread_name("worker".into()).build())
    } else {
        println!("multithreading disabled");
        log("multithreading disabled");
        None
    };
    let combinedip = format!("{}:{}", conf.listen_ip, conf.port);
    let listener = TcpListener::bind(combinedip)?;
    log("opening tcp listener");
    for stream in listener.incoming() {
        if let Ok(mut s) = stream {
            let peer_ip = s.peer_addr().map(|a| a.to_string()).unwrap_or_else(|_| "unknown".to_string());
            let thread_web_dir = Arc::clone(&web_dir);
            match &pool {
                Some(p) => p.execute(move || {
                    log(&format!("got connection from {}", peer_ip));
                    println!("({:?}) got connection from {}", thread::current().id(), peer_ip);
                    let _ = send_web(&mut s, &thread_web_dir);
                }),
                None => {
                    log(&format!("got connection from {}", peer_ip));
                    println!("got connection from {}", peer_ip);
                    let _ = send_web(&mut s, &thread_web_dir);
                }
            }
        }
    }
    Ok(())
}
