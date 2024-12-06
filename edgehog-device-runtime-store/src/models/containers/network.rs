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

//! Container network models.

use diesel::{
    dsl::{exists, BareSelect, Eq, Filter},
    select, Associations, ExpressionMethods, Insertable, QueryDsl, Queryable, Selectable,
};

use crate::{conversions::SqlUuid, schema::containers::networks};

/// Container network with driver configuration.
#[derive(Debug, Clone, Insertable, Queryable, Selectable)]
#[diesel(table_name = crate::schema::containers::networks)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
#[diesel(treat_none_as_default_value = false)]
pub struct Network {
    /// Unique id received from Edgehog.
    pub id: SqlUuid,
    /// Network id returned by the container engine.
    pub local_id: Option<String>,
    /// Status of the network.
    pub created: bool,
    /// Driver to use for the network.
    pub driver: String,
    /// Mark the network as internal.
    pub internal: bool,
    /// Enable ipv6 for the network
    pub enable_ipv6: bool,
}

type NetworkById<'a> = Eq<networks::id, &'a SqlUuid>;
type NetworkFilterById<'a> = Filter<networks::table, NetworkById<'a>>;
type NetworkExists<'a> = BareSelect<exists<Filter<networks::table, NetworkById<'a>>>>;

impl Network {
    /// Returns the filter network table by id.
    pub fn by_id(id: &SqlUuid) -> NetworkById<'_> {
        networks::id.eq(id)
    }

    /// Returns the filtered network table by id.
    pub fn find_id(id: &SqlUuid) -> NetworkFilterById<'_> {
        networks::table.filter(Self::by_id(id))
    }

    /// Returns the network exists query.
    pub fn exists(id: &SqlUuid) -> NetworkExists<'_> {
        select(exists(Self::find_id(id)))
    }
}

/// Container network with driver configuration.
#[derive(Debug, Clone, Insertable, Queryable, Associations, Selectable)]
#[diesel(table_name = crate::schema::containers::network_driver_opts)]
#[diesel(belongs_to(Network))]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
#[diesel(treat_none_as_default_value = false)]
pub struct NetworkDriverOpts {
    /// Id of the network.
    pub network_id: SqlUuid,
    /// Name of the driver option
    pub name: String,
    /// Value of the driver option
    pub value: Option<String>,
}
