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

#![warn(
    missing_docs,
    rustdoc::missing_crate_level_docs,
    clippy::dbg_macro,
    clippy::todo
)]

//! # Edgehog Device Runtime Config
//!
//! Library to manage the configuration for the `edgehog-device-runtime`.
//!
//! It will handle versioning and deserialization of the configuration.

use serde::{Deserialize, Serialize};

mod v1;

/// Configuration, versioned by the `version` key
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "version", rename_all = "lowercase")]
pub enum Config {
    /// `v1` of the configuration
    V1(self::v1::Config),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserialize_config() {
        let string = r#"
        version = "v1"
        "#;

        let config: Config = toml::from_str(string).unwrap();
        let exp = Config::V1(self::v1::Config {});

        assert_eq!(config, exp);
    }
}
