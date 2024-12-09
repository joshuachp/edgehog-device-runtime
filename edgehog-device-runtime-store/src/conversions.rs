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

//! Conversions between rust and SQLITE/database types.

use std::{borrow::Borrow, fmt::Display, ops::Deref};

use diesel::{
    backend::Backend,
    deserialize::{FromSql, FromSqlRow},
    expression::AsExpression,
    serialize::ToSql,
    sql_types::Binary,
};
use uuid::Uuid;

/// Binary serialization of a UUID.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, FromSqlRow, AsExpression)]
#[diesel(sql_type = Binary)]
pub struct SqlUuid(Uuid);

impl Deref for SqlUuid {
    type Target = Uuid;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Borrow<Uuid> for SqlUuid {
    fn borrow(&self) -> &Uuid {
        &self.0
    }
}

impl From<Uuid> for SqlUuid {
    fn from(value: Uuid) -> Self {
        SqlUuid(value)
    }
}

impl Display for SqlUuid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl<B> FromSql<Binary, B> for SqlUuid
where
    B: Backend,
    Vec<u8>: FromSql<Binary, B>,
{
    fn from_sql(bytes: <B as Backend>::RawValue<'_>) -> diesel::deserialize::Result<Self> {
        let data = Vec::<u8>::from_sql(bytes)?;

        Uuid::from_slice(&data).map(SqlUuid).map_err(Into::into)
    }
}

impl<B> ToSql<Binary, B> for SqlUuid
where
    B: Backend,
    [u8; 16]: ToSql<Binary, B>,
{
    fn to_sql<'b>(
        &'b self,
        out: &mut diesel::serialize::Output<'b, '_, B>,
    ) -> diesel::serialize::Result {
        self.as_bytes().to_sql(out)
    }
}
