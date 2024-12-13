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

//! Container models.

use std::fmt::Display;

use rusqlite::{
    types::{FromSql, FromSqlError, ToSqlOutput},
    ToSql,
};
use uuid::Uuid;

/// Container configuration.
#[derive(Debug, Clone)]
pub struct Container {
    /// Unique id received from Edgehog.
    pub id: Uuid,
    /// Container id returned by the container engine.
    pub local_id: Option<String>,
    /// Unique id received from Edgehog.
    pub image_id: Option<Uuid>,
    /// Status of the volume.
    pub status: ContainerStatus,
    /// Container network mode: none, bridge, ...
    pub network_mode: String,
    /// Hostname for the container
    pub hostname: String,
    /// Restart policy
    pub restart_policy: String,
    /// Privileged
    pub privileged: bool,
}

/// Status of a container.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum ContainerStatus {
    /// Received from Edgehog.
    #[default]
    Received = 0,
    /// The container was acknowledged
    Published = 1,
    /// Created on the runtime.
    Created = 2,
    /// Up and running.
    Running = 3,
    /// Stopped or exited.
    Stopped = 4,
}

impl Display for ContainerStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ContainerStatus::Received => write!(f, "Received"),
            ContainerStatus::Published => write!(f, "Published"),
            ContainerStatus::Created => write!(f, "Created"),
            ContainerStatus::Running => write!(f, "Running"),
            ContainerStatus::Stopped => write!(f, "Stopped"),
        }
    }
}

impl From<ContainerStatus> for u8 {
    fn from(value: ContainerStatus) -> Self {
        value as u8
    }
}

impl TryFrom<i64> for ContainerStatus {
    type Error = FromSqlError;

    fn try_from(value: i64) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(ContainerStatus::Received),
            1 => Ok(ContainerStatus::Published),
            2 => Ok(ContainerStatus::Created),
            3 => Ok(ContainerStatus::Running),
            4 => Ok(ContainerStatus::Stopped),
            _ => Err(FromSqlError::OutOfRange(value)),
        }
    }
}

impl FromSql for ContainerStatus {
    fn column_result(value: rusqlite::types::ValueRef<'_>) -> rusqlite::types::FromSqlResult<Self> {
        value.as_i64().and_then(Self::try_from)
    }
}

impl ToSql for ContainerStatus {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(ToSqlOutput::from(u8::from(*self)))
    }
}

/// Environment variables for a container
#[derive(Debug, Clone)]
pub struct ContainerEnv {
    /// [`Container`] id
    pub container_id: Uuid,
    /// Environment variable name and optionally a value
    pub value: String,
}

/// Bind mounts for a container
#[derive(Debug, Clone)]
pub struct ContainerBinds {
    /// [`Container`] id
    pub container_id: Uuid,
    /// Environment variable name and optionally a value
    pub value: String,
}

/// Container port bindings
#[derive(Debug, Clone)]
pub struct ContainerPortBinds {
    /// [`Container`] id
    pub container_id: Uuid,
    /// Container port and optionally protocol
    pub port: String,
    /// Host IP to map the port to
    pub host_ip: Option<String>,
    /// Host port to map the port to
    pub host_port: Option<u16>,
}

#[cfg(test)]
mod tests {
    use super::ContainerStatus;

    #[test]
    fn should_convert_status() {
        let variants = [
            ContainerStatus::Received,
            ContainerStatus::Published,
            ContainerStatus::Created,
            ContainerStatus::Running,
            ContainerStatus::Stopped,
        ];

        for exp in variants {
            let val = i64::from(u8::from(exp));

            let status = ContainerStatus::try_from(val).unwrap();

            assert_eq!(status, exp);
        }
    }
}
