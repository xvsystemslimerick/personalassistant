use base64::{engine::general_purpose::STANDARD, Engine as _};
use family_display_launcher::{
    fetch_snapshot, install_pairing_bundle, load_config, pair_display, DisplayClientConfig,
    PairingBootstrap,
};
use std::{path::Path, sync::Arc, time::Duration};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const LOOPBACK_ADDRESS: &str = "127.0.0.1:4173";
const MAX_HEADER_BYTES: usize = 8 * 1024;
const MAX_ASSET_BYTES: u64 = 2 * 1024 * 1024;

fn main() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("family display runtime unavailable");
    if runtime.block_on(run()).is_err() {
        eprintln!("Family Display could not start safely.");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), ()> {
    let arguments = std::env::args().skip(1).collect::<Vec<_>>();
    match arguments.first().map(String::as_str) {
        Some("pair") if arguments.len() == 7 => {
            pair(
                Path::new(&arguments[1]),
                &arguments[2],
                Path::new(&arguments[3]),
                &arguments[4],
                &arguments[5],
                &arguments[6],
            )
            .await
        }
        Some("serve") if arguments.len() == 3 => {
            serve(Path::new(&arguments[1]), Path::new(&arguments[2])).await
        }
        _ => Err(()),
    }
}

async fn pair(
    config_path: &Path,
    host_url: &str,
    certificate_path: &Path,
    certificate_sha256: &str,
    code: &str,
    display_name: &str,
) -> Result<(), ()> {
    let certificate = std::fs::read(certificate_path).map_err(|_| ())?;
    if certificate.is_empty() || certificate.len() > 16 * 1024 {
        return Err(());
    }
    let bundle = pair_display(
        PairingBootstrap {
            host_url: host_url.to_owned(),
            certificate_sha256: certificate_sha256.to_owned(),
            certificate_der_base64: STANDARD.encode(certificate),
        },
        code,
        display_name,
    )
    .await
    .map_err(|_| ())?;
    install_pairing_bundle(config_path, bundle).map_err(|_| ())
}

async fn serve(config_path: &Path, assets_directory: &Path) -> Result<(), ()> {
    let config = Arc::new(load_config(config_path).map_err(|_| ())?);
    let assets = assets_directory.canonicalize().map_err(|_| ())?;
    if !assets.is_dir() {
        return Err(());
    }
    let listener = tokio::net::TcpListener::bind(LOOPBACK_ADDRESS)
        .await
        .map_err(|_| ())?;
    loop {
        let (socket, peer) = listener.accept().await.map_err(|_| ())?;
        if !peer.ip().is_loopback() {
            continue;
        }
        let config = config.clone();
        let assets = assets.clone();
        tokio::spawn(async move {
            let _ = tokio::time::timeout(
                Duration::from_secs(10),
                handle_loopback(socket, &config, &assets),
            )
            .await;
        });
    }
}

async fn handle_loopback(
    mut socket: tokio::net::TcpStream,
    config: &DisplayClientConfig,
    assets: &Path,
) -> Result<(), ()> {
    let mut request = vec![0_u8; MAX_HEADER_BYTES];
    let mut used = 0;
    loop {
        if used == request.len() {
            return write_response(&mut socket, "400 Bad Request", "text/plain", &[]).await;
        }
        let read = socket.read(&mut request[used..]).await.map_err(|_| ())?;
        if read == 0 {
            return Err(());
        }
        used += read;
        if request[..used].windows(4).any(|value| value == b"\r\n\r\n") {
            break;
        }
    }
    let header = std::str::from_utf8(&request[..used]).map_err(|_| ())?;
    let line = header.split("\r\n").next().ok_or(())?;
    let parts = line.split(' ').collect::<Vec<_>>();
    if parts.len() != 3 || parts[0] != "GET" || parts[2] != "HTTP/1.1" {
        return write_response(&mut socket, "404 Not Found", "text/plain", &[]).await;
    }
    if parts[1] == "/api/v1/snapshot" {
        return match fetch_snapshot(config).await {
            Ok(snapshot) => {
                let body = serde_json::to_vec(&snapshot).map_err(|_| ())?;
                write_response(&mut socket, "200 OK", "application/json", &body).await
            }
            Err(_) => {
                write_response(&mut socket, "503 Service Unavailable", "text/plain", &[]).await
            }
        };
    }
    let (relative, content_type) = if parts[1] == "/" {
        ("index.html", "text/html; charset=utf-8")
    } else if let Some(name) = parts[1].strip_prefix("/assets/") {
        if name.is_empty()
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
        {
            return write_response(&mut socket, "404 Not Found", "text/plain", &[]).await;
        }
        let content_type = if name.ends_with(".css") {
            "text/css; charset=utf-8"
        } else if name.ends_with(".js") {
            "text/javascript; charset=utf-8"
        } else {
            return write_response(&mut socket, "404 Not Found", "text/plain", &[]).await;
        };
        (name, content_type)
    } else {
        return write_response(&mut socket, "404 Not Found", "text/plain", &[]).await;
    };
    let path = if relative == "index.html" {
        assets.join(relative)
    } else {
        assets.join("assets").join(relative)
    };
    let metadata = std::fs::symlink_metadata(&path).map_err(|_| ())?;
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > MAX_ASSET_BYTES
    {
        return write_response(&mut socket, "404 Not Found", "text/plain", &[]).await;
    }
    let body = std::fs::read(path).map_err(|_| ())?;
    write_response(&mut socket, "200 OK", content_type, &body).await
}

async fn write_response(
    socket: &mut tokio::net::TcpStream,
    status: &str,
    content_type: &str,
    body: &[u8],
) -> Result<(), ()> {
    let header = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nContent-Security-Policy: default-src 'self'; object-src 'none'; frame-ancestors 'none'; form-action 'none'\r\nX-Content-Type-Options: nosniff\r\nConnection: close\r\n\r\n",
        body.len()
    );
    socket.write_all(header.as_bytes()).await.map_err(|_| ())?;
    socket.write_all(body).await.map_err(|_| ())?;
    socket.shutdown().await.map_err(|_| ())
}
