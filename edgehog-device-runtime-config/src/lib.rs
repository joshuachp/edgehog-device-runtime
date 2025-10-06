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

pub mod legacy;
mod utils;
pub mod v1;

/// Configuration, versioned by the `version` key
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "version", rename_all = "lowercase")]
pub enum Config {
    /// `v1` of the configuration
    V1(self::v1::Config),
}

/// Compatibility layer with the unversioned configuration.
///
/// This is needed since the old Edgehog Device Runtime configuration didn't have a version field.
#[derive(Debug, Clone, PartialEq)]
pub enum Compatible {
    /// Versioned configuration
    Versioned(Config),
    /// Backwards compatibility to the previous configuration.
    Backwards(self::legacy::Config),
}

impl Compatible {
    /// Deserialize a configuration.
    pub fn deserialize(content: &str) -> Result<Self, toml::de::Error> {
        let value: toml::Table = content.parse().unwrap();

        if value.contains_key("version") {
            let config: Config = value.try_into()?;

            Ok(Compatible::Versioned(config))
        } else {
            let old: self::legacy::Config = value.try_into()?;

            Ok(Compatible::Backwards(old))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserialize_config() {
        let string = r#"
        version = "v1"
        "#;

        let config = Compatible::deserialize(&string).unwrap();
        let exp = Compatible::Versioned(Config::V1(self::v1::Config {}));

        assert_eq!(config, exp);
    }

    #[test]
    fn deserialize_without_version() {
        let string = r#"
        foo = "1"
        "#;

        let config = Compatible::deserialize(&string).unwrap();
        let exp = Compatible::Backwards(self::legacy::Config::default());

        assert_eq!(config, exp);
    }

    #[test]
    fn deserialize_with_version_but_invalid() {
        let string = r#"
        version = "v1"
        foo = "1"
        "#;

        Compatible::deserialize(string).unwrap_err();
    }
}
