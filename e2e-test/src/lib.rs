// This file is part of Edgehog.
//
// Copyright 2022 - 2025 SECO Mind Srl
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//    http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.
//
// SPDX-License-Identifier: Apache-2.0

use eyre::{WrapErr, bail};
use reqwest::Response;
use serde::{Deserialize, Serialize};
use std::future::Future;
use std::time::Duration;
use tracing::error;
use url::Url;

mod config;

#[derive(Serialize, Deserialize)]
struct AstartePayload<T> {
    data: T,
}

/// Retry the future multiple times
async fn retry<F, T, U>(times: usize, mut f: F) -> eyre::Result<U>
where
    F: FnMut() -> T,
    T: Future<Output = eyre::Result<U>>,
{
    let mut interval = tokio::time::interval(Duration::from_secs(1));

    for i in 1..=times {
        match (f)().await {
            Ok(o) => return Ok(o),
            Err(err) => {
                error!("failed retry {i} for: {err}");

                interval.tick().await;
            }
        }
    }

    bail!("to many attempts")
}

async fn wait_for_cluster(api_url: &Url) -> eyre::Result<()> {
    let appengine = api_url.join("/appengine/health")?.to_string();
    let pairing = api_url.join("/pairing/health")?.to_string();

    retry(20, move || {
        let appengine = appengine.clone();
        let pairing = pairing.clone();

        async move {
            reqwest::get(&appengine)
                .await
                .and_then(Response::error_for_status)
                .wrap_err("appengine call failed")?;

            reqwest::get(&pairing)
                .await
                .and_then(Response::error_for_status)
                .wrap_err("pairing call failed")
        }
    })
    .await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::config::{EdgehogConfig, setup};

    use super::*;

    use edgehog_device_runtime::telemetry::status::hardware_info::HardwareInfo;
    use edgehog_device_runtime::telemetry::status::os_release::{OsInfo, OsRelease};
    use edgehog_device_runtime::telemetry::status::runtime_info::{RUNTIME_INFO, RuntimeInfo};
    use eyre::OptionExt;
    use pretty_assertions::assert_eq;
    use serde::de::DeserializeOwned;
    use tracing::debug;

    async fn get_interface_data<T>(config: &EdgehogConfig, interface: &str) -> eyre::Result<T>
    where
        T: DeserializeOwned,
    {
        let url = config
            .astarte_api_url
            .join(&format!(
                "/appengine/v1/{}/devices/{}/interfaces/{interface}",
                config.realm_name, config.device_id
            ))?
            .to_string();

        retry(20, || async {
            let body = reqwest::Client::new()
                .get(&url)
                .bearer_auth(&config.token)
                .send()
                .await
                .wrap_err_with(|| format!("get interface for {interface} failed"))?
                .error_for_status()?
                .text()
                .await
                .wrap_err("couldn't get body text")?;

            debug!("response: {body}");

            serde_json::from_str(&body)
                .wrap_err_with(|| format!("coudln't deserialize interface {interface}"))
        })
        .await
    }

    #[tokio::test]
    async fn os_info_test() -> eyre::Result<()> {
        let config = setup().await?;

        let info = OsRelease::read()
            .await
            .ok_or_eyre("couldn't read os release")?;

        let os_info_from_astarte: AstartePayload<OsInfo> =
            get_interface_data(&config, "io.edgehog.devicemanager.OSInfo").await?;

        assert_eq!(os_info_from_astarte.data, info.os_info);

        Ok(())
    }

    #[tokio::test]
    async fn hardware_info_test() -> eyre::Result<()> {
        let config = setup().await?;

        let local_hw = HardwareInfo::read().await;

        let astarte_hw = get_interface_data::<AstartePayload<HardwareInfo>>(
            &config,
            "io.edgehog.devicemanager.HardwareInfo",
        )
        .await?
        .data;

        assert_eq!(local_hw, astarte_hw);

        Ok(())
    }

    #[tokio::test]
    async fn runtime_info_test() -> eyre::Result<()> {
        let config = setup().await?;

        let local_rt = RUNTIME_INFO;

        let astarte_rt = get_interface_data::<AstartePayload<RuntimeInfo>>(
            &config,
            "io.edgehog.devicemanager.RuntimeInfo",
        )
        .await?
        .data;

        assert_eq!(local_rt, astarte_rt);

        Ok(())
    }
}
