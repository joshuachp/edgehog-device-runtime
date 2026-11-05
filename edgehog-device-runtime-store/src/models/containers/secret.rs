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

//! Container secret models.

use std::borrow::Cow;
use std::fmt::{Debug, Display};

use diesel::backend::Backend;
use diesel::deserialize::{FromSql, FromSqlRow};
use diesel::dsl::exists;
use diesel::expression::AsExpression;
use diesel::prelude::Insertable;
use diesel::serialize::{IsNull, ToSql};
use diesel::sql_types::SmallInt;
use diesel::sqlite::Sqlite;
use diesel::{HasQuery, select};

use crate::conversions::SqlUuid;
use crate::models::{ExistsFilterById, QueryModel};
use crate::schema::containers::secrets;

/// Container secret to be mounted in a container.
#[derive(Clone, Insertable, HasQuery, PartialEq, Eq)]
#[diesel(table_name = crate::schema::containers::secrets)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
pub struct Secret<'a> {
    /// Unique id received from Edgehog.
    pub id: SqlUuid,
    /// Status of the secret.
    pub status: SecretStatus,
    /// Raw binary content of the secret.
    pub content: Cow<'a, [u8]>,
}

impl Debug for Secret<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Secret")
            .field("id", &self.id)
            .field("status", &self.status)
            .finish_non_exhaustive()
    }
}

impl QueryModel for Secret<'_> {
    type Table = secrets::table;

    type Id = secrets::id;

    type ExistsQuery<'a> = ExistsFilterById<'a, Self::Table, Self::Id>;

    fn exists(id: &SqlUuid) -> Self::ExistsQuery<'_> {
        select(exists(Self::find_id(id)))
    }
}

/// Status of a secret.
#[repr(u8)]
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, FromSqlRow, AsExpression,
)]
#[diesel(sql_type = SmallInt)]
pub enum SecretStatus {
    /// Received from Edgehog.
    #[default]
    Received = 0,
    /// The secret was acknowledged.
    Published = 1,
}

impl Display for SecretStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SecretStatus::Received => write!(f, "Received"),
            SecretStatus::Published => write!(f, "Published"),
        }
    }
}

impl From<SecretStatus> for i16 {
    fn from(value: SecretStatus) -> Self {
        (value as u8).into()
    }
}

impl TryFrom<i16> for SecretStatus {
    type Error = String;

    fn try_from(value: i16) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(SecretStatus::Received),
            1 => Ok(SecretStatus::Published),
            _ => Err(format!("unrecognized status value {value}")),
        }
    }
}

impl FromSql<SmallInt, Sqlite> for SecretStatus {
    fn from_sql(bytes: <Sqlite as Backend>::RawValue<'_>) -> diesel::deserialize::Result<Self> {
        let value = i16::from_sql(bytes)?;

        Self::try_from(value).map_err(Into::into)
    }
}

impl ToSql<SmallInt, Sqlite> for SecretStatus {
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

    use super::SecretStatus;

    #[test]
    fn should_convert_status() {
        let variants = [SecretStatus::Received, SecretStatus::Published];

        for exp in variants {
            let val = i16::from(exp);

            let status = SecretStatus::try_from(val).unwrap();

            assert_eq!(status, exp);
        }
    }
}
