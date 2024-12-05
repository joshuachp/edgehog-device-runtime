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

//! Models for all the resources.

use std::fmt::Display;

use diesel::{
    backend::Backend,
    deserialize::{FromSql, FromSqlRow},
    expression::AsExpression,
    prelude::*,
    serialize::{IsNull, ToSql},
    sql_types::Integer,
    sqlite::Sqlite,
};

use crate::converions::SqlUuid;

/// Container image with the authentication to pull it.
#[derive(Insertable, Queryable, Selectable)]
#[diesel(table_name = crate::schema::containers::images)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
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

/// Container network with driver configuration.
#[derive(Insertable, Queryable, Selectable)]
#[diesel(table_name = crate::schema::containers::networks)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
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

/// Container network with driver configuration.
#[derive(Insertable, Queryable, Associations, Selectable)]
#[diesel(table_name = crate::schema::containers::network_driver_opts)]
#[diesel(belongs_to(Network))]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
pub struct NetworkDriverOpts {
    /// Id of the network.
    pub network_id: SqlUuid,
    /// Name of the driver option
    pub name: String,
    /// Value of the driver option
    pub value: Option<String>,
}

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

/// Container volume with driver configuration.
#[derive(Insertable, Queryable, Associations, Selectable)]
#[diesel(table_name = crate::schema::containers::volume_driver_opts)]
#[diesel(belongs_to(Volume))]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
pub struct VolumeDriverOpts {
    /// Id of the volume.
    pub volume_id: SqlUuid,
    /// Name of the driver option
    pub name: String,
    /// Value of the driver option
    pub value: Option<String>,
}

/// Container configuration.
#[derive(Insertable, Queryable, Selectable)]
#[diesel(table_name = crate::schema::containers::containers)]
#[diesel(belongs_to(Image))]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
pub struct Container {
    /// Unique id received from Edgehog.
    pub id: SqlUuid,
    /// Container id returned by the container engine.
    pub local_id: Option<String>,
    /// Unique id received from Edgehog.
    pub image_id: Option<SqlUuid>,
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

/// Status of a container.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, FromSqlRow, AsExpression)]
#[diesel(sql_type = Integer)]
pub enum ContainerStatus {
    /// Received from Edgehog.
    Received = 0,
    /// Created on the runtime.
    Created = 1,
    /// Up and running.
    Running = 2,
    /// Stopped or exited.
    Stopped = 3,
}

impl Display for ContainerStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ContainerStatus::Received => write!(f, "Received"),
            ContainerStatus::Created => write!(f, "Created"),
            ContainerStatus::Running => write!(f, "Running"),
            ContainerStatus::Stopped => write!(f, "Stopped"),
        }
    }
}

impl From<ContainerStatus> for i32 {
    fn from(value: ContainerStatus) -> Self {
        (value as u8).into()
    }
}

impl<B> FromSql<Integer, B> for ContainerStatus
where
    B: Backend,
    i32: FromSql<Integer, B>,
{
    fn from_sql(bytes: <B as Backend>::RawValue<'_>) -> diesel::deserialize::Result<Self> {
        let value = i32::from_sql(bytes)?;

        match value {
            0 => Ok(ContainerStatus::Received),
            1 => Ok(ContainerStatus::Created),
            2 => Ok(ContainerStatus::Running),
            3 => Ok(ContainerStatus::Stopped),
            _ => Err(format!("unrecognized container status {value}").into()),
        }
    }
}

impl ToSql<Integer, Sqlite> for ContainerStatus
where
    i32: ToSql<Integer, Sqlite>,
{
    fn to_sql<'b>(
        &'b self,
        out: &mut diesel::serialize::Output<'b, '_, Sqlite>,
    ) -> diesel::serialize::Result {
        let val = i32::from(*self);

        out.set_value(val);

        Ok(IsNull::No)
    }
}

/// Missing image for a container
#[derive(Insertable, Queryable, Selectable)]
#[diesel(table_name = crate::schema::containers::container_missing_images)]
#[diesel(belongs_to(Container))]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
pub struct ContainerMissingImage {
    /// [`Container`] id
    pub container_id: SqlUuid,
    /// [`Image`] id
    pub image_id: SqlUuid,
}

/// Networks used by a container
#[derive(Insertable, Queryable, Selectable)]
#[diesel(table_name = crate::schema::containers::container_networks)]
#[diesel(belongs_to(Container))]
#[diesel(belongs_to(Network))]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
pub struct ContainerNetwork {
    /// [`Container`] id
    pub container_id: SqlUuid,
    /// [`Network`] id
    pub network_id: SqlUuid,
}

/// Missing image for a container
#[derive(Insertable, Queryable, Selectable)]
#[diesel(table_name = crate::schema::containers::container_missing_networks)]
#[diesel(belongs_to(Container))]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
pub struct ContainerMissingNetwork {
    /// [`Container`] id
    pub container_id: SqlUuid,
    /// [`Network`] id
    pub network_id: SqlUuid,
}

/// Volumes used by a container
#[derive(Insertable, Queryable, Selectable)]
#[diesel(table_name = crate::schema::containers::container_volumes)]
#[diesel(belongs_to(Container))]
#[diesel(belongs_to(Volume))]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
pub struct ContainerVolume {
    /// [`Container`] id
    pub container_id: SqlUuid,
    /// [`Volume`] id
    pub volume_id: SqlUuid,
}

/// Missing image for a container
#[derive(Insertable, Queryable, Selectable)]
#[diesel(table_name = crate::schema::containers::container_missing_volumes)]
#[diesel(belongs_to(Container))]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
pub struct ContainerMissingVolume {
    /// [`Container`] id
    pub container_id: SqlUuid,
    /// [`Volume`] id
    pub volume_id: SqlUuid,
}

/// Environment variables for a container
#[derive(Insertable, Queryable, Selectable)]
#[diesel(table_name = crate::schema::containers::container_env)]
#[diesel(belongs_to(Container))]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
pub struct ContainerEnv {
    /// [`Container`] id
    pub container_id: SqlUuid,
    /// Environment variable name and optionally a value
    pub value: String,
}

/// Bind mounts for a container
#[derive(Insertable, Queryable, Selectable)]
#[diesel(table_name = crate::schema::containers::container_binds)]
#[diesel(belongs_to(Container))]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
pub struct ContainerBinds {
    /// [`Container`] id
    pub container_id: SqlUuid,
    /// Environment variable name and optionally a value
    pub value: String,
}

/// Container port bindings
#[derive(Insertable, Queryable, Selectable)]
#[diesel(table_name = crate::schema::containers::container_port_bindings)]
#[diesel(belongs_to(Container))]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
pub struct ContainerPortBinds {
    /// [`Container`] id
    pub container_id: SqlUuid,
    /// Container port and optionally protocol
    pub port: String,
    /// Host IP to map the port to
    pub host_ip: Option<String>,
    /// Host port to map the port to
    pub host_port: Option<String>,
}

/// Container deployment
#[derive(Insertable, Queryable, Selectable)]
#[diesel(table_name = crate::schema::containers::deployments)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
pub struct Deployment {
    /// Unique id received from Edgehog.
    pub id: SqlUuid,
    /// Status of the deployment.
    pub status: DeploymentStatus,
}

/// Status of a deployment.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, FromSqlRow, AsExpression)]
#[diesel(sql_type = Integer)]
pub enum DeploymentStatus {
    /// Received from Edgehog.
    Stopped = 0,
    /// Stopped or exited.
    Started = 1,
}

impl Display for DeploymentStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DeploymentStatus::Stopped => write!(f, "Stopped"),
            DeploymentStatus::Started => write!(f, "Started"),
        }
    }
}

impl From<DeploymentStatus> for i32 {
    fn from(value: DeploymentStatus) -> Self {
        (value as u8).into()
    }
}

impl<B> FromSql<Integer, B> for DeploymentStatus
where
    B: Backend,
    i32: FromSql<Integer, B>,
{
    fn from_sql(bytes: <B as Backend>::RawValue<'_>) -> diesel::deserialize::Result<Self> {
        let value = i32::from_sql(bytes)?;

        match value {
            0 => Ok(DeploymentStatus::Started),
            3 => Ok(DeploymentStatus::Stopped),
            _ => Err(format!("unrecognized deployment status {value}").into()),
        }
    }
}

impl ToSql<Integer, Sqlite> for DeploymentStatus
where
    i32: ToSql<Integer, Sqlite>,
{
    fn to_sql<'b>(
        &'b self,
        out: &mut diesel::serialize::Output<'b, '_, Sqlite>,
    ) -> diesel::serialize::Result {
        let val = i32::from(*self);

        out.set_value(val);

        Ok(IsNull::No)
    }
}

/// Container deployment
#[derive(Insertable, Queryable, Selectable)]
#[diesel(table_name = crate::schema::containers::deployment_containers)]
#[diesel(belongs_to(Deployment))]
#[diesel(belongs_to(Container))]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
pub struct DeploymentContainer {
    /// [`Deployment`] id
    pub deployment_id: SqlUuid,
    /// [`Container`] id
    pub container_id: SqlUuid,
}

/// Missing image for a container
#[derive(Insertable, Queryable, Selectable)]
#[diesel(table_name = crate::schema::containers::deployment_missing_containers)]
#[diesel(belongs_to(DeploymentContainer))]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
pub struct DeploymentMissingCOntainer {
    /// [`Deployment`] id
    pub deployment_id: SqlUuid,
    /// [`Container`] id
    pub container_id: SqlUuid,
}
