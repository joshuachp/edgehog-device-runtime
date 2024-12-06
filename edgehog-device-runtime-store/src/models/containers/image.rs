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

use diesel::{
    dsl::{exists, BareSelect, Eq, Filter},
    select, ExpressionMethods, Insertable, QueryDsl, Queryable, Selectable,
};

use crate::{conversions::SqlUuid, schema::containers::images};

/// Container image with the authentication to pull it.
#[derive(Debug, Clone, Insertable, Queryable, Selectable)]
#[diesel(table_name = crate::schema::containers::images)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
#[diesel(treat_none_as_default_value = false)]
pub struct Image {
    /// Unique id received from Edgehog.
    pub id: SqlUuid,
    /// Image id returned by the container engine.
    pub local_id: Option<String>,
    /// Status of the image.
    pub pulled: bool,
    /// Image reference to be pulled.
    ///
    /// It's in the form of: `docker.io/library/postgres:15-alpine`
    pub reference: String,
    /// Base64 encoded JSON for the registry auth.
    pub registry_auth: Option<String>,
}

type ImageById<'a> = Eq<images::id, &'a SqlUuid>;
type ImageFilterById<'a> = Filter<images::table, ImageById<'a>>;
type ImageExists<'a> = BareSelect<exists<Filter<images::table, ImageById<'a>>>>;

impl Image {
    /// Returns the filter image table by id.
    pub fn by_id(id: &SqlUuid) -> ImageById<'_> {
        images::id.eq(id)
    }

    /// Returns the filtered image table by id.
    pub fn find_id(id: &SqlUuid) -> ImageFilterById<'_> {
        images::table.filter(Self::by_id(id))
    }

    /// Returns the image exists query.
    pub fn exists(id: &SqlUuid) -> ImageExists<'_> {
        select(exists(Self::find_id(id)))
    }
}
