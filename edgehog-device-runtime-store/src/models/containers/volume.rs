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

use diesel::{
    dsl::{exists, BareSelect, Eq, Filter},
    select, Associations, ExpressionMethods, Insertable, QueryDsl, Queryable, Selectable,
};

use crate::{conversions::SqlUuid, schema::containers::volumes};

/// Container volume with driver configuration.
#[derive(Insertable, Queryable, Selectable)]
#[diesel(table_name = crate::schema::containers::volumes)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
pub struct Volume {
    /// Unique id received from Edgehog.
    pub id: SqlUuid,
    /// Status of the volume.
    pub created: bool,
    /// Driver to use for the volume.
    pub driver: String,
}

type VolumeById<'a> = Eq<volumes::id, &'a SqlUuid>;
type VolumeFilterById<'a> = Filter<volumes::table, VolumeById<'a>>;
type VolumeExists<'a> = BareSelect<exists<Filter<volumes::table, VolumeById<'a>>>>;

impl Volume {
    /// Returns the filter volume table by id.
    pub fn by_id(id: &SqlUuid) -> VolumeById<'_> {
        volumes::id.eq(id)
    }

    /// Returns the filtered volume table by id.
    pub fn find_id(id: &SqlUuid) -> VolumeFilterById<'_> {
        volumes::table.filter(Self::by_id(id))
    }

    /// Returns the volume exists query.
    pub fn exists(id: &SqlUuid) -> VolumeExists<'_> {
        select(exists(Self::find_id(id)))
    }
}

/// Container volume with driver configuration.
#[derive(Insertable, Queryable, Associations, Selectable)]
#[diesel(table_name = crate::schema::containers::volume_driver_opts)]
#[diesel(belongs_to(Volume))]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
#[diesel(treat_none_as_default_value = false)]
pub struct VolumeDriverOpts {
    /// Id of the volume.
    pub volume_id: SqlUuid,
    /// Name of the driver option
    pub name: String,
    /// Value of the driver option
    pub value: Option<String>,
}
