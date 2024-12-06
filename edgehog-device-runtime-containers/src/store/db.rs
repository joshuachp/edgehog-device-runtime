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

//! Persistent stores of the request issued by Astarte and resources created.

use diesel::{insert_into, RunQueryDsl};
use diesel::{Connection, Table};
use edgehog_store::conversions::SqlUuid;
use edgehog_store::models::containers::container::{
    ContainerBinds, ContainerEnv, ContainerNetwork, ContainerPortBinds, ContainerStatus,
    ContainerVolume, HostPort,
};
use edgehog_store::{
    db::{self, Result},
    models::containers::{
        container::{
            Container, ContainerMissingImage, ContainerMissingNetwork, ContainerMissingVolume,
        },
        image::Image,
        network::{Network, NetworkDriverOpts},
        volume::Volume,
    },
    schema::containers::{
        container_binds, container_env, container_missing_images, container_missing_networks,
        container_missing_volumes, container_networks, container_port_bindings, container_volumes,
        containers, images, network_driver_opts, networks, volumes,
    },
};
use itertools::Itertools;
use tracing::debug;

use crate::container::PortBindingMap;
use crate::requests::container::CreateContainer;

/// Handle to persist the state.
///
/// The file is a new line delimited JSON.
#[derive(Debug)]
pub(crate) struct StateStore {
    handle: db::Handle,
}

impl StateStore {
    /// Creates a new state store
    pub(crate) fn new(handle: db::Handle) -> Self {
        Self { handle }
    }

    /// Stores the image received from the CreateRequest
    pub(crate) async fn create_image(&self, image: Image) -> Result<()> {
        self.handle
            .for_write(|writer| {
                insert_into(images::table)
                    .values(image)
                    .on_conflict(images::id)
                    .do_nothing()
                    .execute(writer)?;

                Ok(())
            })
            .await
    }

    /// Stores the network received from the CreateRequest
    pub(crate) async fn create_network(
        &self,
        network: Network,
        opts: Vec<NetworkDriverOpts>,
    ) -> Result<()> {
        self.handle
            .for_write(|writer| {
                writer.transaction(|writer| -> Result<()> {
                    insert_into(networks::table)
                        .values(network)
                        .on_conflict(networks::id)
                        .do_nothing()
                        .execute(writer)?;

                    // FIXME: the on_conflict(network_driver_opts::table.primary_key()) doesn't work
                    //        with batch insert
                    for opt in opts {
                        insert_into(network_driver_opts::table)
                            .values(opt)
                            .on_conflict(network_driver_opts::table.primary_key())
                            .do_nothing()
                            .execute(writer)?;
                    }

                    Ok(())
                })?;

                Ok(())
            })
            .await
    }

    /// Stores the volume received from the CreateRequest
    pub(crate) async fn create_volume(&self, volume: Volume) -> Result<()> {
        self.handle
            .for_write(|writer| {
                insert_into(volumes::table)
                    .values(volume)
                    .on_conflict(volumes::id)
                    .do_nothing()
                    .execute(writer)?;

                Ok(())
            })
            .await
    }

    /// Stores the container received from the CreateRequest
    ///
    /// The related resources
    pub(crate) async fn create_container(
        &self,
        value: &CreateContainer,
        port_bindings: PortBindingMap<String>,
    ) -> Result<()> {
        let mut container = Container::from(value);
        let image_id = SqlUuid::from(*value.image_id);
        let networks = value
            .network_ids
            .iter()
            .map(|id| SqlUuid::from(**id))
            .collect_vec();
        let volumes = value
            .volume_ids
            .iter()
            .map(|id| SqlUuid::from(**id))
            .collect_vec();

        let envs = value.env.clone();
        let binds = value.binds.clone();

        self.handle
            .for_write(move |writer| {
                writer.transaction(move |writer| {
                    let image_exists: bool = Image::exists(&image_id).get_result(writer)?;

                    if !image_exists {
                        debug!("image is missing, storing image_id into container_missing_images");

                        insert_into(container_missing_images::table)
                            .values(ContainerMissingImage {
                                container_id: container.id,
                                image_id,
                            })
                            .on_conflict_do_nothing()
                            .execute(writer)?;

                        container.image_id.take();
                    }

                    insert_into(containers::table)
                        .values(&container)
                        .on_conflict(containers::id)
                        .do_nothing()
                        .execute(writer)?;

                    for env in envs {
                        insert_into(container_env::table)
                            .values(ContainerEnv {
                                container_id: container.id,
                                value: env,
                            })
                            .on_conflict_do_nothing()
                            .execute(writer)?;
                    }

                    for bind in binds {
                        insert_into(container_binds::table)
                            .values(ContainerBinds {
                                container_id: container.id,
                                value: bind,
                            })
                            .on_conflict_do_nothing()
                            .execute(writer)?;
                    }

                    let iter = port_bindings
                        .iter()
                        .flat_map(|(port, bindings)| bindings.iter().map(move |bind| (port, bind)));

                    for (port, bind) in iter {
                        insert_into(container_port_bindings::table)
                            .values(ContainerPortBinds {
                                container_id: container.id,
                                port: port.to_string(),
                                host_ip: bind.host_ip.clone(),
                                host_port: bind.host_port.map(HostPort),
                            })
                            .on_conflict_do_nothing()
                            .execute(writer)?;
                    }

                    for network_id in networks {
                        let network_exists: bool =
                            Network::exists(&network_id).get_result(writer)?;

                        if !network_exists {
                            insert_into(container_missing_networks::table)
                                .values(ContainerMissingNetwork {
                                    container_id: container.id,
                                    network_id,
                                })
                                .on_conflict_do_nothing()
                                .execute(writer)?;

                            continue;
                        }

                        insert_into(container_networks::table)
                            .values(ContainerNetwork {
                                container_id: container.id,
                                network_id,
                            })
                            .on_conflict_do_nothing()
                            .execute(writer)?;
                    }

                    for volume_id in volumes {
                        let volume_exists: bool = Volume::exists(&volume_id).get_result(writer)?;

                        if !volume_exists {
                            insert_into(container_missing_volumes::table)
                                .values(ContainerMissingVolume {
                                    container_id: container.id,
                                    volume_id,
                                })
                                .on_conflict_do_nothing()
                                .execute(writer)?;

                            continue;
                        }

                        insert_into(container_volumes::table)
                            .values(ContainerVolume {
                                container_id: container.id,
                                volume_id,
                            })
                            .on_conflict_do_nothing()
                            .execute(writer)?;
                    }

                    Ok(())
                })
            })
            .await
    }
}

impl From<&CreateContainer> for Container {
    fn from(
        CreateContainer {
            id,
            image_id,
            hostname,
            restart_policy,
            network_mode,
            privileged,
            network_ids: _,
            volume_ids: _,
            image: _,
            env: _,
            binds: _,
            port_bindings: _,
        }: &CreateContainer,
    ) -> Self {
        Self {
            id: SqlUuid::from(**id),
            local_id: None,
            image_id: Some(SqlUuid::from(**image_id)),
            status: ContainerStatus::default(),
            network_mode: network_mode.to_string(),
            hostname: hostname.to_string(),
            restart_policy: restart_policy.to_string(),
            privileged: *privileged,
        }
    }
}
