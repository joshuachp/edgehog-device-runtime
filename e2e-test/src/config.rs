// This file is part of Edgehog.
//
// Copyright 2026 SECO Mind Srl
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.
//
// SPDX-License-Identifier: Apache-2.0

use std::path::PathBuf;
use std::process::{Stdio, abort};
use std::sync::Arc;

use astarte_device_sdk::store;
use edgehog_device_runtime::data::astarte_device_sdk_lib::AstarteDeviceSdkConfigOptions;
use edgehog_device_runtime::data::connect_store;
use edgehog_device_runtime::ota::config::OtaConfig;
use edgehog_device_runtime::{AstarteLibrary, DeviceManagerOptions, Runtime};
use eyre::{Context, eyre};
use serde::Deserialize;
use tempdir::TempDir;
use tokio::process::Command;
use tokio::sync::Mutex;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;
use url::Url;

use crate::wait_for_cluster;

#[derive(Debug, Deserialize)]
pub(crate) struct EdgehogConfig {
    /// Astarte API base url
    pub(crate) astarte_api_url: Url,
    /// Astarte realm to use
    pub(crate) realm_name: String,
    /// Astarte device id to send data from.
    pub(crate) device_id: String,
    /// The test device credentials secret
    pub(crate) credentials_secret: String,
    /// Token with access to the Astarte APIs
    pub(crate) token: String,
    /// Ignore SSL errors when talking to MQTT Broker.
    pub(crate) ignore_ssl: bool,
    /// Interface directory for the Device.
    pub(crate) interface_dir: PathBuf,
}

impl EdgehogConfig {
    pub(crate) fn pairing_url(&self) -> eyre::Result<Url> {
        self.astarte_api_url
            .join("pairing")
            .wrap_err("couldn't get pairing url")
    }
}

pub(crate) struct Setup {
    config: Arc<EdgehogConfig>,
    process: std::thread::JoinHandle<()>,
}

pub(crate) async fn setup() -> eyre::Result<Arc<EdgehogConfig>> {
    static CONFIG: Mutex<Option<Setup>> = Mutex::const_new(None);

    let mut setup = CONFIG.lock().await;
    match setup.as_ref() {
        Some(setup) => return Ok(Arc::clone(&setup.config)),
        None => {}
    }

    tracing_subscriber::fmt()
        .with_test_writer()
        .with_max_level(tracing::Level::TRACE)
        .init();

    rustls::crypto::aws_lc_rs::default_provider()
        .install_default()
        .map_err(|_| eyre!("couldn't set default provider"))?;

    let config: EdgehogConfig = config::Config::builder()
        .add_source(config::File::with_name("../.config/e2e"))
        .add_source(config::Environment::with_prefix("E2E"))
        .build()?
        .try_deserialize()?;

    wait_for_cluster(&config.astarte_api_url).await?;

    let config = Arc::new(config);

    let process = std::thread::spawn(|| {
        let mut child = std::process::Command::new("../target/debug/edgehog-device-runtime")
            .args(["--config=../.config/runtime.toml"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .stdin(Stdio::null())
            .spawn()
            .wrap_err("couldn't execute edgehog_device_runtime")
            .unwrap();

        if !child.wait().unwrap().success() {
            abort();
        }
    });

    setup.replace(Setup {
        config: Arc::clone(&config),
        process,
    });

    Ok(config)
}
