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
    Connection, OptionalExtension, ToSql, Transaction,
};
use uuid::Uuid;

use crate::{db::HandleError, models::include_query};

/// Container configuration.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
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

impl Container {
    fn create(&self, transaction: &mut Transaction) -> Result<(), HandleError> {
        let Container {
            id,
            local_id,
            image_id,
            status,
            network_mode,
            hostname,
            restart_policy,
            privileged,
        } = self;

        transaction
            .prepare_cached(include_query!("write/container/insert_container.sql"))?
            .execute((
                id,
                local_id,
                image_id,
                status,
                network_mode,
                hostname,
                restart_policy,
                privileged,
            ))?;

        Ok(())
    }

    fn by_id(connection: &Connection, id: &Uuid) -> Result<Option<Self>, HandleError> {
        connection
            .prepare_cached(include_query!("read/container/by_id_container.sql"))?
            .query_row((id,), |r| {
                let container = Container {
                    id: r.get("id")?,
                    local_id: r.get("local_id")?,
                    image_id: r.get("image_id")?,
                    status: r.get("status")?,
                    network_mode: r.get("network_mode")?,
                    hostname: r.get("hostname")?,
                    restart_policy: r.get("restart_policy")?,
                    privileged: r.get("privileged")?,
                };

                Ok(container)
            })
            .optional()
            .map_err(HandleError::Query)
    }

    fn create_missing_image(
        &mut self,
        transaction: &mut Transaction,
        image_id: Uuid,
    ) -> Result<(), HandleError> {
        let container_image_id = self.image_id.take();
        debug_assert_eq!(container_image_id, Some(image_id));

        transaction
            .prepare_cached(include_query!(
                "write/container/insert_container_missing_image.sql"
            ))?
            .execute((self.id, image_id))?;

        Ok(())
    }
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
    use tempfile::TempDir;

    use crate::db::Handle;

    use super::*;

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

    #[tokio::test]
    async fn should_store_and_get_by_id() {
        let tmpfile = TempDir::with_suffix("store-container").unwrap();
        let mut handle = Handle::open(tmpfile.path().join("database.db"))
            .await
            .unwrap();

        let container = Container {
            id: Uuid::new_v4(),
            local_id: Some("local_id".to_string()),
            image_id: Some(Uuid::new_v4()),
            status: ContainerStatus::Created,
            network_mode: "bridge".to_string(),
            hostname: "hostname".to_string(),
            restart_policy: "unless-stopped".to_string(),
            privileged: true,
        };

        handle
            .for_write_transaction({
                let container = container.clone();
                move |conn| container.create(conn)
            })
            .await
            .unwrap();

        let res = handle
            .for_read(move |conn| Container::by_id(conn, &container.id))
            .await
            .unwrap()
            .unwrap();

        assert_eq!(res, container);
    }

    #[tokio::test]
    async fn should_not_error_on_multiple_stores() {
        let tmpfile = TempDir::with_suffix("multi-store-container").unwrap();
        let mut handle = Handle::open(tmpfile.path().join("database.db"))
            .await
            .unwrap();

        let container = Container {
            id: Uuid::new_v4(),
            local_id: Some("local_id".to_string()),
            image_id: Some(Uuid::new_v4()),
            status: ContainerStatus::Created,
            network_mode: "bridge".to_string(),
            hostname: "hostname".to_string(),
            restart_policy: "unless-stopped".to_string(),
            privileged: true,
        };

        handle
            .for_write_transaction({
                let mut container = container.clone();
                move |conn| {
                    container.create(conn)?;

                    // Creating the image should ignore the changes
                    container.status = ContainerStatus::Received;

                    container.create(conn)
                }
            })
            .await
            .unwrap();

        let res = handle
            .for_read(move |conn| Container::by_id(conn, &container.id))
            .await
            .unwrap()
            .unwrap();

        // Should be equal to the first image
        assert_eq!(res, container);
    }

    #[tokio::test]
    async fn should_insert_container_missing_image() {
        let tmpfile = TempDir::with_suffix("multi-store-container").unwrap();
        let mut handle = Handle::open(tmpfile.path().join("database.db"))
            .await
            .unwrap();

        let container = Container {
            id: Uuid::new_v4(),
            local_id: Some("local_id".to_string()),
            image_id: Some(Uuid::new_v4()),
            status: ContainerStatus::Created,
            network_mode: "bridge".to_string(),
            hostname: "hostname".to_string(),
            restart_policy: "unless-stopped".to_string(),
            privileged: true,
        };

        handle
            .for_write_transaction({
                let mut container = container.clone();
                move |conn| {
                    container.create_missing_image(conn, container.image_id.unwrap())?;

                    let mut container_withot_image = container.clone();

                    container_withot_image.image_id = None;

                    assert_eq!(container, container_withot_image);

                    Ok(())
                }
            })
            .await
            .unwrap();

        handle
            .for_read(move |conn| {
                conn.query_row(
                    "
SELECT * FROM
    container_missing_images
WHERE 
    container_missing_images.container_id = ?
    AND container_missing_images.image_id = ?;",
                    (container.id, container.image_id),
                    |_r| Ok(()),
                )
                .unwrap();
                Ok(())
            })
            .await
            .unwrap();
    }
}
