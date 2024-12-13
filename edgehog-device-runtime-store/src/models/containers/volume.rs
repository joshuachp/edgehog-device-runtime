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

//! Container volume models.

use std::fmt::Display;

use rusqlite::{
    types::{FromSql, FromSqlError, ToSqlOutput},
    ToSql,
};
use uuid::Uuid;

/// Container volume with driver configuration.
#[derive(Debug, Clone)]
pub struct Volume {
    /// Unique id received from Edgehog.
    pub id: Uuid,
    /// Status of the volume.
    pub status: VolumeStatus,
    /// Driver to use for the volume.
    pub driver: String,
}

/// Container volume with driver configuration.
#[derive(Debug, Clone)]
pub struct VolumeDriverOpts {
    /// Id of the volume.
    pub volume_id: Uuid,
    /// Name of the driver option
    pub name: String,
    /// Value of the driver option
    pub value: Option<String>,
}

/// Status of a volume.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum VolumeStatus {
    /// Received from Edgehog.
    #[default]
    Received = 0,
    /// The volume was acknowledged
    Published = 1,
    /// Created on the runtime.
    Created = 2,
}

impl Display for VolumeStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VolumeStatus::Received => write!(f, "Received"),
            VolumeStatus::Published => write!(f, "Published"),
            VolumeStatus::Created => write!(f, "Created"),
        }
    }
}

impl From<VolumeStatus> for u8 {
    fn from(value: VolumeStatus) -> Self {
        value as u8
    }
}

impl TryFrom<i64> for VolumeStatus {
    type Error = FromSqlError;

    fn try_from(value: i64) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(VolumeStatus::Received),
            1 => Ok(VolumeStatus::Published),
            2 => Ok(VolumeStatus::Created),
            _ => Err(FromSqlError::OutOfRange(value)),
        }
    }
}

impl FromSql for VolumeStatus {
    fn column_result(value: rusqlite::types::ValueRef<'_>) -> rusqlite::types::FromSqlResult<Self> {
        value.as_i64().and_then(Self::try_from)
    }
}

impl ToSql for VolumeStatus {
    fn to_sql(&self) -> rusqlite::Result<rusqlite::types::ToSqlOutput<'_>> {
        Ok(ToSqlOutput::from(u8::from(*self)))
    }
}

#[cfg(test)]
mod tests {
    use super::VolumeStatus;

    #[test]
    fn should_convert_status() {
        let variants = [
            VolumeStatus::Received,
            VolumeStatus::Published,
            VolumeStatus::Created,
        ];

        for exp in variants {
            let val = i64::from(u8::from(exp));

            let status = VolumeStatus::try_from(val).unwrap();

            assert_eq!(status, exp);
        }
    }
}
