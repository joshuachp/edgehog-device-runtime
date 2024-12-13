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

//! Container network models.

use std::fmt::Display;

use rusqlite::{
    types::{FromSql, FromSqlError, ToSqlOutput},
    ToSql,
};
use uuid::Uuid;

/// Container network with driver configuration.
#[derive(Debug, Clone)]
pub struct Network {
    /// Unique id received from Edgehog.
    pub id: Uuid,
    /// Network id returned by the container engine.
    pub local_id: Option<String>,
    /// Status of the network.
    pub status: NetworkStatus,
    /// Driver to use for the network.
    pub driver: String,
    /// Mark the network as internal.
    pub internal: bool,
    /// Enable ipv6 for the network
    pub enable_ipv6: bool,
}

/// Container network with driver configuration.
#[derive(Debug, Clone)]
pub struct NetworkDriverOpts {
    /// Id of the network.
    pub network_id: Uuid,
    /// Name of the driver option
    pub name: String,
    /// Value of the driver option
    pub value: Option<String>,
}

/// Status of a network.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum NetworkStatus {
    /// Received from Edgehog.
    #[default]
    Received = 0,
    /// The network was acknowledged
    Published = 1,
    /// Created on the runtime.
    Created = 2,
}

impl Display for NetworkStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NetworkStatus::Received => write!(f, "Received"),
            NetworkStatus::Published => write!(f, "Published"),
            NetworkStatus::Created => write!(f, "Created"),
        }
    }
}

impl From<NetworkStatus> for u8 {
    fn from(value: NetworkStatus) -> Self {
        value as u8
    }
}

impl TryFrom<i64> for NetworkStatus {
    type Error = FromSqlError;

    fn try_from(value: i64) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(NetworkStatus::Received),
            1 => Ok(NetworkStatus::Published),
            2 => Ok(NetworkStatus::Created),
            _ => Err(FromSqlError::OutOfRange(value)),
        }
    }
}

impl FromSql for NetworkStatus {
    fn column_result(value: rusqlite::types::ValueRef<'_>) -> rusqlite::types::FromSqlResult<Self> {
        value.as_i64().and_then(Self::try_from)
    }
}

impl ToSql for NetworkStatus {
    fn to_sql(&self) -> rusqlite::Result<rusqlite::types::ToSqlOutput<'_>> {
        Ok(ToSqlOutput::from(u8::from(*self)))
    }
}

#[cfg(test)]
mod tests {
    use super::NetworkStatus;

    #[test]
    fn should_convert_status() {
        let variants = [
            NetworkStatus::Received,
            NetworkStatus::Published,
            NetworkStatus::Created,
        ];

        for exp in variants {
            let val = i64::from(u8::from(exp));

            let status = NetworkStatus::try_from(val).unwrap();

            assert_eq!(status, exp);
        }
    }
}
