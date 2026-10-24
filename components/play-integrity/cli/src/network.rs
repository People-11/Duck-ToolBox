// SPDX-License-Identifier: GPL-3.0-only
// Modified 2026-10-23: bounded HTTPS transport with embedded CA roots.
use crate::files;
use anyhow::{Result, bail};
use reqwest::{Client, Method};
use rustls::{ClientConfig, RootCertStore};
use std::{path::Path, time::Duration};

pub async fn fetch(args: &[String]) -> Result<()> {
    let mut output = None;
    let mut log = None;
    let mut headers = Vec::new();
    let mut head = false;
    let mut url = None;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "-O" | "-o" | "-T" | "--header" => {
                let flag = &args[index];
                index += 1;
                let value = args
                    .get(index)
                    .ok_or_else(|| anyhow::anyhow!("missing {flag} argument"))?;
                match flag.as_str() {
                    "-O" => output = Some(value.clone()),
                    "-o" => log = Some(value.clone()),
                    "--header" => headers.push(value.clone()),
                    _ => {}
                }
            }
            "--spider" => head = true,
            "-q" | "-S" => {}
            value if value.starts_with('-') => bail!("unsupported fetch option: {value}"),
            value => url = Some(value.to_owned()),
        }
        index += 1;
    }
    let url = reqwest::Url::parse(&url.ok_or_else(|| anyhow::anyhow!("missing URL"))?)?;
    if url.scheme() != "https" {
        bail!("only HTTPS downloads are permitted");
    }
    let mut roots = RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let mut tls = ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();
    tls.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
    let client = Client::builder()
        .https_only(true)
        .tls_backend_preconfigured(tls)
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(20))
        .build()?;
    let mut request = client.request(if head { Method::HEAD } else { Method::GET }, url);
    for header in headers {
        let (key, value) = header
            .split_once(':')
            .ok_or_else(|| anyhow::anyhow!("invalid header"))?;
        request = request.header(key.trim(), value.trim());
    }
    let mut response = request.send().await?.error_for_status()?;
    let mut report = format!("HTTP {}\n", response.status());
    for (key, value) in response.headers() {
        let key = if key.as_str() == "last-modified" {
            "Last-Modified"
        } else {
            key.as_str()
        };
        report.push_str(&format!("{key}: {}\n", value.to_str().unwrap_or_default()));
    }
    if let Some(path) = log {
        files::atomic(Path::new(&path), report.as_bytes())?;
    } else {
        eprint!("{report}");
    }
    if head {
        return Ok(());
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        if body.len() + chunk.len() > 4 * 1024 * 1024 {
            bail!("download exceeds 4 MiB");
        }
        body.extend_from_slice(&chunk);
    }
    if let Some(path) = output {
        files::atomic(Path::new(&path), &body)?;
    } else {
        use std::io::Write;
        std::io::stdout().write_all(&body)?;
    }
    Ok(())
}
