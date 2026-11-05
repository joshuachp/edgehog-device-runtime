// This file is part of Edgehog.
//
// Copyright 2026 SECO Mind Srl
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

//! Container secret reference models.

use std::borrow::Cow;
use std::fmt::Display;

use diesel::associations::Associations;
use diesel::backend::Backend;
use diesel::deserialize::{FromSql, FromSqlRow};
use diesel::dsl::exists;
use diesel::expression::AsExpression;
use diesel::prelude::Insertable;
use diesel::serialize::{IsNull, ToSql};
use diesel::sql_types::SmallInt;
use diesel::sqlite::Sqlite;
use diesel::{HasQuery, select};

use crate::conversions::{SqlU32, SqlUuid};
use crate::models::containers::container::Container;
use crate::models::containers::secret::Secret;
use crate::models::{ExistsFilterById, QueryModel};
use crate::schema::containers::secret_refs;

/// Container secret reference instructing how to mount a secret in a container.
#[derive(Debug, Clone, Insertable, HasQuery, Associations, PartialEq, Eq)]
#[diesel(table_name = crate::schema::containers::secret_refs)]
#[diesel(belongs_to(Secret<'_>))]
#[diesel(belongs_to(Container<'_>))]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
#[diesel(treat_none_as_default_value = false)]
pub struct SecretRef<'a> {
    /// Unique id received from Edgehog.
    pub id: SqlUuid,
    /// Status of the secret reference.
    pub status: SecretRefStatus,
    /// Id of the referenced [`Secret`].
    pub secret_id: SqlUuid,
    /// Id of the [`Container`] using this secret reference.
    pub container_id: SqlUuid,
    /// Target path or file name for the secret.
    pub target: Cow<'a, str>,
    /// Numeric UID that owns the secret file.
    pub uid: Option<SqlU32>,
    /// Numeric GID that owns the secret file.
    pub gid: Option<SqlU32>,
    /// Permissions for the secret file.
    pub mode: Option<SqlU32>,
}

impl QueryModel for SecretRef<'_> {
    type Table = secret_refs::table;

    type Id = secret_refs::id;

    type ExistsQuery<'a> = ExistsFilterById<'a, Self::Table, Self::Id>;

    fn exists(id: &SqlUuid) -> Self::ExistsQuery<'_> {
        select(exists(Self::find_id(id)))
    }
}

/// Status of a secret reference.
#[repr(u8)]
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, FromSqlRow, AsExpression,
)]
#[diesel(sql_type = SmallInt)]
pub enum SecretRefStatus {
    /// Received from Edgehog.
    #[default]
    Received = 0,
    /// The secret reference was acknowledged.
    Published = 1,
}

impl Display for SecretRefStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SecretRefStatus::Received => write!(f, "Received"),
            SecretRefStatus::Published => write!(f, "Published"),
        }
    }
}

impl From<SecretRefStatus> for i16 {
    fn from(value: SecretRefStatus) -> Self {
        (value as u8).into()
    }
}

impl TryFrom<i16> for SecretRefStatus {
    type Error = String;

    fn try_from(value: i16) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(SecretRefStatus::Received),
            1 => Ok(SecretRefStatus::Published),
            _ => Err(format!("unrecognized status value {value}")),
        }
    }
}

impl FromSql<SmallInt, Sqlite> for SecretRefStatus {
    fn from_sql(bytes: <Sqlite as Backend>::RawValue<'_>) -> diesel::deserialize::Result<Self> {
        let value = i16::from_sql(bytes)?;

        Self::try_from(value).map_err(Into::into)
    }
}

impl ToSql<SmallInt, Sqlite> for SecretRefStatus {
    fn to_sql<'b>(
        &'b self,
        out: &mut diesel::serialize::Output<'b, '_, Sqlite>,
    ) -> diesel::serialize::Result {
        let val = i16::from(*self);

        out.set_value(i32::from(val));

        Ok(IsNull::No)
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::SecretRefStatus;

    #[test]
    fn should_convert_status() {
        let variants = [SecretRefStatus::Received, SecretRefStatus::Published];

        for exp in variants {
            let val = i16::from(exp);

            let status = SecretRefStatus::try_from(val).unwrap();

            assert_eq!(status, exp);
        }
    }
}
