// This file is part of Edgehog.
//
// Copyright 2025 SECO Mind Srl
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

use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;

use cfg_if::cfg_if;
use serde::{Deserialize, Serialize};
use url::Url;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(flatten)]
    pub astarte_library: AstarteLibrary,
    pub containers: ContainersConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "kebab-case", tag = "astarte_library")]
pub enum AstarteLibrary {
    AstarteDeviceSdk {
        astarte_device_sdk: DeviceSdk,
    },
    AstarteMessageHub {
        astarte_message_hub: AstarteMessageHub,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct DeviceSdk {
    /// The Astarte realm the device belongs to.
    pub realm: String,
    /// A unique ID for the device.
    pub device_id: String,
    /// Paring token or credential secret used to authenticate with astarte
    #[serde(flatten)]
    pub credentials: SdkCredentials,
    /// Url to the Astarte pairing API
    pub pairing_url: Url,
    /// Ignores SSL error from the Astarte broker.
    #[serde(default)]
    pub ignore_ssl: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum SdkCredentials {
    /// The credentials secret used to authenticate with Astarte.
    CredentialsSecret(String),
    /// Token used to register the device.
    PairingToken(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AstarteMessageHub {
    /// The Endpoint of the Astarte Message Hub to connect to
    endpoint: Url,
}

/// Configuration for the container service.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ContainersConfig {
    /// Flag to make the container service is required
    #[serde(default)]
    required: bool,
    /// Maximum number of retries for the initialization of the service
    #[serde(default = "ContainersConfig::default_max_retries")]
    max_retries: usize,
}

impl ContainersConfig {
    /// Maximum number of retries for the initialization of the service
    pub const MAX_INIT_RETRIES: usize = 10;

    const fn default_max_retries() -> usize {
        Self::MAX_INIT_RETRIES
    }
}

impl Default for ContainersConfig {
    fn default() -> Self {
        Self {
            required: false,
            max_retries: Self::default_max_retries(),
        }
    }
}

/// Configuration for the [`EdgehogService`](crate::service::EdgehogService)
#[derive(Debug, Clone, PartialEq, Default, Deserialize, Serialize)]
pub struct Service {
    /// Flag to enable the service
    #[serde(default)]
    pub enabled: bool,
    /// Listener for the service
    #[serde(default)]
    pub listener: Listener,
}

/// Listener for the service
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Listener {
    /// Unix domain socket
    Unix(PathBuf),
    /// TCP socket
    Socket(SocketAddr),
}

impl Default for Listener {
    fn default() -> Self {
        cfg_if! {
            if #[cfg(unix)] {
                let path = std::env::var("XDG_RUNTIME_DIR")
                    .map(PathBuf::from)
                    .unwrap_or_else(|_| PathBuf::from("/tmp"))
                    .join("edgehog-device-runtime.sock");

                Listener::Unix(path)
            } else {
                Listener::Socket(std::net::SocketAddrV4::new(std::net::Ipv4Addr::LOCALHOST, 50052))
            }
        }
    }
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct OtaConfig {
    #[serde(default)]
    pub reboot: Reboot,
    #[serde(default)]
    pub streaming: bool,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Reboot {
    #[default]
    Default,
    External,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TelemetryInterface {
    pub interface_name: String,
    #[serde(default)]
    pub enabled: bool,
    #[serde(
        default = "TelemetryInterface::default_period",
        with = "crate::utils::duration_from_secs"
    )]
    pub period: Duration,
}

impl TelemetryInterface {
    pub const DEFAULT_PERIOD: Duration = Duration::from_secs(60);

    const fn default_period() -> Duration {
        Self::DEFAULT_PERIOD
    }
}

#[cfg(test)]
mod tests {
    use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};

    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    #[cfg(unix)]
    fn get_default_unix() {
        let root = std::env::var("XDG_RUNTIME_DIR").unwrap_or("/tmp".to_string());
        let mut path = PathBuf::from(root);

        path.push("edgehog-device-runtime.sock");

        let exp = Service {
            enabled: false,
            listener: Listener::Unix(path),
        };

        assert_eq!(Service::default(), exp);
    }

    #[test]
    fn should_deserialize_config() {
        let file = r#"
        [listener]
        unix = "/foo"
        "#;

        let exp = Service {
            enabled: false,
            listener: Listener::Unix(PathBuf::from("/foo")),
        };

        let res: Service = toml::from_str(file).unwrap();

        assert_eq!(res, exp);
    }

    #[test]
    fn should_serialize_config() {
        let exp = r#"enabled = true

[listener]
socket = "0.0.0.0:8080"
"#;

        let conf = Service {
            enabled: true,
            listener: Listener::Socket(SocketAddr::V4(SocketAddrV4::new(
                Ipv4Addr::UNSPECIFIED,
                8080,
            ))),
        };

        let res = toml::to_string_pretty(&conf).unwrap();

        assert_eq!(res, exp);
    }
}
