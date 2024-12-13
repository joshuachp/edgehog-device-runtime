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

//! Container image models.

use std::{fmt::Display, i64};

use rusqlite::{
    types::{FromSql, FromSqlError, ToSqlOutput},
    Connection, OptionalExtension, ToSql, Transaction,
};
use tracing::debug;
use uuid::Uuid;

use crate::{db::HandleError, models::include_query};

/// Container image with the authentication to pull it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Image {
    /// Unique id received from Edgehog.
    pub id: Uuid,
    /// Image id returned by the container engine.
    pub local_id: Option<String>,
    /// Status of the image.
    pub status: ImageStatus,
    /// Image reference to be pulled.
    ///
    /// It's in the form of: `docker.io/library/postgres:15-alpine`
    pub reference: String,
    /// Base64 encoded JSON for the registry auth.
    pub registry_auth: Option<String>,
}

impl Image {
    fn create(&self, conn: &mut Transaction) -> Result<(), HandleError> {
        let Self {
            id,
            local_id,
            status,
            reference,
            registry_auth,
        } = self;

        conn.prepare_cached(include_query!("write/image/insert_image.sql"))?
            .execute((id, local_id, status, reference, registry_auth))?;

        Ok(())
    }

    fn by_id(connection: &Connection, id: &Uuid) -> Result<Option<Self>, HandleError> {
        connection
            .prepare_cached(include_query!("read/image/by_id_image.sql"))?
            .query_row((id,), |r| {
                let image = Image {
                    id: r.get("id")?,
                    local_id: r.get("local_id")?,
                    status: r.get("status")?,
                    reference: r.get("reference")?,
                    registry_auth: r.get("registry_auth")?,
                };

                Ok(image)
            })
            .optional()
            .map_err(HandleError::Query)
    }

    fn update_missing_images(&self, transaction: &mut Transaction) -> Result<(), HandleError> {
        let count = transaction.execute(
            include_query!("write/image/update_container_missing_image.sql"),
            (self.id,),
        )?;

        debug!("Updated {count} containers with missing images");

        Ok(())
    }
}

/// Status of an image.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum ImageStatus {
    /// Received from Edgehog.
    #[default]
    Received = 0,
    /// The image was acknowledged
    Published = 1,
    /// Created on the runtime.
    Pulled = 2,
}

impl Display for ImageStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ImageStatus::Received => write!(f, "Received"),
            ImageStatus::Published => write!(f, "Published"),
            ImageStatus::Pulled => write!(f, "Pulled"),
        }
    }
}

impl From<ImageStatus> for u8 {
    fn from(value: ImageStatus) -> Self {
        value as u8
    }
}

impl TryFrom<i64> for ImageStatus {
    type Error = FromSqlError;

    fn try_from(value: i64) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(ImageStatus::Received),
            1 => Ok(ImageStatus::Published),
            2 => Ok(ImageStatus::Pulled),
            _ => Err(FromSqlError::OutOfRange(value)),
        }
    }
}

impl FromSql for ImageStatus {
    fn column_result(value: rusqlite::types::ValueRef<'_>) -> rusqlite::types::FromSqlResult<Self> {
        value.as_i64().and_then(Self::try_from)
    }
}

impl ToSql for ImageStatus {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        let value = u8::from(*self);

        Ok(ToSqlOutput::from(value))
    }
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;
    use uuid::Uuid;

    use crate::{db::Handle, models::containers::image::Image};

    use super::*;

    #[test]
    fn should_convert_status() {
        let variants = [
            ImageStatus::Received,
            ImageStatus::Published,
            ImageStatus::Pulled,
        ];

        for exp in variants {
            let val: i64 = u8::from(exp).into();

            let status = ImageStatus::try_from(val).unwrap();

            assert_eq!(status, exp);
        }
    }

    #[tokio::test]
    async fn should_store_and_get_by_id() {
        let tmpfile = TempDir::with_suffix("store-image").unwrap();
        let mut handle = Handle::open(tmpfile.path().join("database.db"))
            .await
            .unwrap();

        let image = Image {
            id: Uuid::new_v4(),
            local_id: Some("local_id".to_string()),
            status: ImageStatus::Pulled,
            reference: "docker.io/library/postgres:15".to_string(),
            registry_auth: None,
        };

        handle
            .for_write_transaction({
                let image = image.clone();
                move |conn| image.create(conn)
            })
            .await
            .unwrap();

        let res = handle
            .for_read(move |conn| Image::by_id(conn, &image.id))
            .await
            .unwrap()
            .unwrap();

        assert_eq!(res, image);
    }

    #[tokio::test]
    async fn should_not_error_on_multiple_stores() {
        let tmpfile = TempDir::with_suffix("multi-store-image").unwrap();
        let mut handle = Handle::open(tmpfile.path().join("database.db"))
            .await
            .unwrap();

        let image = Image {
            id: Uuid::new_v4(),
            local_id: Some("local_id".to_string()),
            status: ImageStatus::Pulled,
            reference: "docker.io/library/postgres:15".to_string(),
            registry_auth: None,
        };

        handle
            .for_write_transaction({
                let mut image = image.clone();
                move |conn| {
                    image.create(conn)?;

                    // Creating the image should ignore the changes
                    image.status = ImageStatus::Received;

                    image.create(conn)
                }
            })
            .await
            .unwrap();

        let res = handle
            .for_read(move |conn| Image::by_id(conn, &image.id))
            .await
            .unwrap()
            .unwrap();

        // Should be equal to the first image
        assert_eq!(res, image);
    }
}
