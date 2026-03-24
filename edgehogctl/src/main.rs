use std::io::IsTerminal;
use std::path::PathBuf;

use clap::Parser;
use color_eyre::Section;
use eyre::{OptionExt, ensure, eyre};
use tracing::{info, instrument};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use url::Url;
use uuid::Uuid;

#[derive(Debug, clap::Parser)]
struct Cli {
    #[clap(subcommand)]
    command: Command,
}

#[derive(Debug, Clone, clap::Subcommand)]
enum Command {
    Ft(FileTransfer),
}

#[derive(Debug, Clone, clap::Args)]
struct FileTransfer {
    #[arg(long)]
    compression: bool,
    #[arg(long)]
    file_size: Option<u64>,
    /// Url to download the file from
    url: Url,
    /// Path of the file on the file system to download
    path: PathBuf,
}

impl FileTransfer {
    #[instrument]
    async fn transfer(self) -> eyre::Result<()> {
        let content = tokio::fs::read(&self.path).await?;

        let digent = aws_lc_rs::digest::digest(&aws_lc_rs::digest::SHA256, &content);

        let compression = self
            .compression
            .then_some("tar.gz")
            .unwrap_or_default()
            .to_string();
        let file_name = self
            .path
            .file_name()
            .unwrap()
            .to_str()
            .ok_or_eyre("non utf8 file name")?
            .to_string();

        let file_size_bytes = self.file_size.unwrap_or(content.len() as u64);

        let data = ServerToDevice {
            id: Uuid::new_v4(),
            url: self.url,
            http_header_key: Vec::new(),
            http_header_value: Vec::new(),
            compression,
            file_name,
            ttl_seconds: 0,
            file_mode: 0,
            user_id: -1,
            group_id: -1,
            progress: false,
            digest: format!("sha256:{}", hex::encode(digent.as_ref())),
            file_size_bytes,
            destination_type: "storage".to_string(),
            destination: "storage".to_string(),
        };

        let client = reqwest::Client::builder()
            .use_preconfigured_tls(edgehog_tls::config()?)
            .build()?;

        let url = "http://api.astarte.localhost/appengine/v1/test/devices/WQQ81So7Q9-DpUZ9I_IAQg/interfaces/io.edgehog.devicemanager.fileTransfer.posix.ServerToDevice/request";

        let token = tokio::process::Command::new("astartectl")
            .args(["utils", "gen-jwt", "all-realm-apis"])
            .output()
            .await?;

        ensure!(token.status.success(), "error in astartectl command");

        let token = str::from_utf8(&token.stdout)?.trim();

        let resp = client
            .post(url)
            .bearer_auth(token)
            .json(&ApiData { data })
            .send()
            .await?;

        if let Err(err) = resp.error_for_status_ref() {
            let resp: serde_json::Value = resp.json().await?;

            let body = serde_json::to_string_pretty(&resp)?;

            return Err(err).with_note(|| body);
        }

        info!("request sent to device");

        Ok(())
    }
}

#[tokio::main]
async fn main() -> eyre::Result<()> {
    color_eyre::install()?;

    let cli = Cli::parse();

    let fmt = tracing_subscriber::fmt::layer();
    #[cfg(not(windows))]
    let fmt = fmt.with_ansi(std::io::stdout().is_terminal());

    tracing_subscriber::registry()
        .with(fmt)
        .with(
            tracing_subscriber::EnvFilter::builder()
                .with_default_directive(tracing::Level::DEBUG.into())
                .from_env_lossy(),
        )
        .with(tracing_error::ErrorLayer::default())
        .try_init()?;

    // Set default crypto provider
    rustls::crypto::aws_lc_rs::default_provider()
        .install_default()
        .map_err(|_| eyre!("failed to install default crypto provider"))?;

    match cli.command {
        Command::Ft(ft) => {
            ft.transfer().await?;
        }
    }

    Ok(())
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ServerToDevice {
    id: Uuid,
    url: Url,
    http_header_key: Vec<String>,
    http_header_value: Vec<String>,
    compression: String,
    file_name: String,
    ttl_seconds: i64,
    file_mode: i64,
    user_id: i64,
    group_id: i64,
    progress: bool,
    digest: String,
    file_size_bytes: u64,
    destination_type: String,
    destination: String,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
struct ApiData<T> {
    data: T,
}
