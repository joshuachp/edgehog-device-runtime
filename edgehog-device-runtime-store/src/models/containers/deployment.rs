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

//! Container deployment models.

use std::fmt::Display;

use rusqlite::{
    types::{FromSql, FromSqlError, ToSqlOutput},
    ToSql,
};
use uuid::Uuid;

/// Container deployment
#[derive(Debug, Clone, Copy)]
pub struct Deployment {
    /// Unique id received from Edgehog.
    pub id: Uuid,
    /// Status of the deployment.
    pub status: DeploymentStatus,
}

/// Status of a deployment.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum DeploymentStatus {
    /// Received from Edgehog.
    #[default]
    Received = 0,
    /// The deployment was acknowledged
    Published = 1,
    /// Up and running.
    Started = 2,
    /// Was stopped.
    Stopped = 3,
}

impl Display for DeploymentStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DeploymentStatus::Received => write!(f, "Received"),
            DeploymentStatus::Published => write!(f, "Published"),
            DeploymentStatus::Started => write!(f, "Started"),
            DeploymentStatus::Stopped => write!(f, "Stopped"),
        }
    }
}

impl From<DeploymentStatus> for u8 {
    fn from(value: DeploymentStatus) -> Self {
        value as u8
    }
}

impl TryFrom<i64> for DeploymentStatus {
    type Error = FromSqlError;

    fn try_from(value: i64) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(DeploymentStatus::Received),
            1 => Ok(DeploymentStatus::Published),
            2 => Ok(DeploymentStatus::Started),
            3 => Ok(DeploymentStatus::Stopped),
            _ => Err(FromSqlError::OutOfRange(value)),
        }
    }
}

impl FromSql for DeploymentStatus {
    fn column_result(value: rusqlite::types::ValueRef<'_>) -> rusqlite::types::FromSqlResult<Self> {
        value.as_i64().and_then(Self::try_from)
    }
}

impl ToSql for DeploymentStatus {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(ToSqlOutput::from(u8::from(*self)))
    }
}

#[cfg(test)]
mod tests {
    use super::DeploymentStatus;

    #[test]
    fn should_convert_status() {
        let variants = [
            DeploymentStatus::Received,
            DeploymentStatus::Published,
            DeploymentStatus::Started,
            DeploymentStatus::Stopped,
        ];

        for exp in variants {
            let val = i64::from(u8::from(exp));

            let status = DeploymentStatus::try_from(val).unwrap();

            assert_eq!(status, exp);
        }
    }
}
