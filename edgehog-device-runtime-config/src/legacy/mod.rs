// This file is part of Edgehog.
//
// Copyright 2025 Seco Mind Srl
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

//! Old unversioned configuration.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use url::Url;

/// Configuration file
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Config {
    pub astarte_library: Option<AstarteLibrary>,

    pub astarte_device_sdk: Option<DeviceSdkArgs>,
    pub astarte_message_hub: Option<MsgHubArgs>,

    pub containers: Option<crate::v1::ContainersConfig>,

    pub service: Option<crate::v1::Service>,

    pub ota: Option<crate::v1::OtaConfig>,

    pub interfaces_directory: Option<PathBuf>,
    pub store_directory: Option<PathBuf>,
    pub download_directory: Option<PathBuf>,

    pub telemetry_config: Option<Vec<crate::v1::TelemetryInterface>>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum AstarteLibrary {
    #[default]
    AstarteDeviceSdk,
    AstarteMessageHub,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct MsgHubArgs {
    /// The Endpoint of the Astarte Message Hub to connect to
    endpoint: Option<Url>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct DeviceSdkArgs {
    /// The Astarte realm the device belongs to.
    pub realm: Option<String>,
    /// A unique ID for the device.
    pub device_id: Option<String>,
    /// The credentials secret used to authenticate with Astarte.
    pub credentials_secret: Option<String>,
    /// Token used to register the device.
    pub pairing_token: Option<String>,
    /// Url to the Astarte pairing API
    pub pairing_url: Option<Url>,
    /// Ignores SSL error from the Astarte broker.
    pub ignore_ssl: Option<bool>,
}
