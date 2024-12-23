// This file is part of Edgehog.
//
// Copyright 2024 SECO Mind Srl
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//   http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.
//
// SPDX-License-Identifier: Apache-2.0

use std::fmt::Debug;
use std::path::Path;

use color_eyre::{
    eyre::{eyre, WrapErr},
    owo_colors::OwoColorize,
    Section, SectionExt,
};
use reqwest::{header, Url};
use serde::{Deserialize, Serialize};
use tracing::info;

use crate::cli::AstarteConfig;

#[derive(Debug)]
pub struct ApiClient {
    token: String,
    url: Url,
}

impl ApiClient {
    pub fn new(
        astarte: &AstarteConfig,
        token: String,
        appengine_url: String,
    ) -> color_eyre::Result<Self> {
        let url = format!(
            "{}/v1/{}/devices/{}/interfaces",
            appengine_url, astarte.realm, astarte.device_id
        )
        .parse()?;

        Ok(Self { token, url })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Data<T> {
    data: T,
}

impl ApiClient {
    async fn send<T>(&self, interface: &str, path: &str, data: T) -> color_eyre::Result<()>
    where
        T: Serialize + Debug,
    {
        let url = self.interface_url(interface, path)?;

        let res = reqwest::Client::new()
            .post(url)
            .bearer_auth(&self.token)
            .header(header::ACCEPT, "application/json")
            .json(&Data { data: &data })
            .send()
            .await?;

        let status = res.status();

        let text = res.text().await.wrap_err("couldn't get response body")?;

        if !status.is_success() {
            let err = eyre!("HTTP status {status} for ({interface}{path})")
                .with_section(move || format!("{:#?}", data.dimmed()).header("Request:"))
                .with_section(move || format!("{}", text.red()).header("Response:"));

            return Err(err);
        }

        info!("response: {text}");

        Ok(())
    }

    fn interface_url(&self, interface: &str, path: &str) -> Result<Url, color_eyre::eyre::Error> {
        let mut url = self.url.clone();
        url.path_segments_mut()
            .map_err(|_| eyre!("couldn't get the url path {}", self.url))?
            .push(interface)
            .push(path.strip_prefix('/').unwrap_or(path));
        Ok(url)
    }

    pub async fn read(&self, path: &Path) -> color_eyre::Result<()> {
        let content = tokio::fs::read_to_string(path).await?;

        let values: File = serde_json::from_str(&content)?;

        for v in values.events {
            self.send(&v.interface, &v.path, v.data).await?;
        }

        Ok(())
    }

    pub async fn print_curl(&self, path: &Path) -> color_eyre::Result<()> {
        let content = tokio::fs::read_to_string(path).await?;

        let values: File = serde_json::from_str(&content)?;

        println!("#!/usr/bin/env bash");
        println!();
        println!("set -exEuo pipefail");
        println!();

        for v in values.events {
            self.print_send(v)?;
        }

        Ok(())
    }

    fn print_send(&self, data: InterfaceData) -> color_eyre::Result<()> {
        println!(
            r#"curl -v -X POST -H "Accept: application/json" -H "Content-Type: application/json" \"#
        );
        println!(r#"    -H "Authorization: Bearer $TOKEN" \"#);
        println!(
            r#"    --data '{}' \"#,
            serde_json::to_string(&Data { data: &data.data })?
        );

        println!("    '{}'", self.interface_url(&data.interface, &data.path)?);

        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct InterfaceData {
    interface: String,
    path: String,
    data: serde_json::Value,
}

#[derive(Debug, Serialize, Deserialize)]
struct File {
    events: Vec<InterfaceData>,
}
