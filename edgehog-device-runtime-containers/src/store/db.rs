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

use diesel::{delete, insert_or_ignore_into, ExpressionMethods, RunQueryDsl};
use diesel::{update, QueryDsl};
use edgehog_store::conversions::SqlUuid;
use edgehog_store::models::containers::container::{
    ContainerBinds, ContainerEnv, ContainerNetwork, ContainerPortBinds, ContainerStatus,
    ContainerVolume, HostPort,
};
use edgehog_store::models::containers::image::ImageStatus;
use edgehog_store::models::containers::network::NetworkStatus;
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
use tracing::{debug, instrument};

use crate::container::PortBindingMap;
use crate::requests::container::CreateContainer;
use crate::requests::image::CreateImage;
use crate::requests::network::CreateNetwork;

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
    #[instrument(skip_all, fields(%image.id))]
    pub(crate) async fn create_image(&self, image: CreateImage) -> Result<()> {
        let image = Image::from(image);

        self.handle
            .for_write_transaction(move |writer| {
                insert_or_ignore_into(images::table)
                    .values(&image)
                    .execute(writer)?;

                update(containers::table)
                    .set(containers::image_id.eq(image.id))
                    .filter(
                        containers::id.eq_any(
                            ContainerMissingImage::find_by_image(&image.id)
                                .select(container_missing_images::container_id),
                        ),
                    )
                    .execute(writer)?;

                delete(container_missing_images::table)
                    .filter(container_missing_images::image_id.eq(image.id))
                    .execute(writer)?;

                Ok(())
            })
            .await
    }

    /// Stores the network received from the CreateRequest
    #[instrument(skip_all, fields(%network.id))]
    pub(crate) async fn create_network(
        &self,
        create_network: CreateNetwork,
        opts: Vec<NetworkDriverOpts>,
    ) -> Result<()> {
        let network = Network::from(&create_network);

        self.handle
            .for_write_transaction(move |writer| {
                insert_or_ignore_into(networks::table)
                    .values(&network)
                    .execute(writer)?;

                insert_or_ignore_into(network_driver_opts::table)
                    .values(opts)
                    .execute(writer)?;

                insert_or_ignore_into(container_networks::table)
                    .values(ContainerMissingNetwork::find_by_network(&network.id))
                    .execute(writer)?;

                delete(ContainerMissingNetwork::find_by_network(&network.id)).execute(writer)?;

                Ok(())
            })
            .await
    }

    /// Stores the volume received from the CreateRequest
    pub(crate) async fn create_volume(&self, volume: Volume) -> Result<()> {
        self.handle
            .for_write(|writer| {
                insert_or_ignore_into(volumes::table)
                    .values(volume)
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
            .for_write_transaction(move |writer| {
                let image_exists: bool = Image::exists(&image_id).get_result(writer)?;

                if !image_exists {
                    debug!("image is missing, storing image_id into container_missing_images");

                    insert_or_ignore_into(container_missing_images::table)
                        .values(ContainerMissingImage {
                            container_id: container.id,
                            image_id,
                        })
                        .execute(writer)?;

                    container.image_id.take();
                }

                insert_or_ignore_into(containers::table)
                    .values(&container)
                    .execute(writer)?;

                let envs = envs
                    .into_iter()
                    .map(|value| ContainerEnv {
                        container_id: container.id,
                        value,
                    })
                    .collect_vec();
                insert_or_ignore_into(container_env::table)
                    .values(envs)
                    .execute(writer)?;

                let binds = binds
                    .into_iter()
                    .map(|value| ContainerBinds {
                        container_id: container.id,
                        value,
                    })
                    .collect_vec();
                insert_or_ignore_into(container_binds::table)
                    .values(binds)
                    .execute(writer)?;

                let prt_bindings = port_bindings
                    .iter()
                    .flat_map(|(port, bindings)| {
                        bindings.iter().map(move |bind| ContainerPortBinds {
                            container_id: container.id,
                            port: port.to_string(),
                            host_ip: bind.host_ip.clone(),
                            host_port: bind.host_port.map(HostPort),
                        })
                    })
                    .collect_vec();
                insert_or_ignore_into(container_port_bindings::table)
                    .values(prt_bindings)
                    .execute(writer)?;

                for network_id in networks {
                    let network_exists: bool = Network::exists(&network_id).get_result(writer)?;

                    if !network_exists {
                        insert_or_ignore_into(container_missing_networks::table)
                            .values(ContainerMissingNetwork {
                                container_id: container.id,
                                network_id,
                            })
                            .execute(writer)?;

                        continue;
                    }

                    insert_or_ignore_into(container_networks::table)
                        .values(ContainerNetwork {
                            container_id: container.id,
                            network_id,
                        })
                        .execute(writer)?;
                }

                for volume_id in volumes {
                    let volume_exists: bool = Volume::exists(&volume_id).get_result(writer)?;

                    if !volume_exists {
                        insert_or_ignore_into(container_missing_volumes::table)
                            .values(ContainerMissingVolume {
                                container_id: container.id,
                                volume_id,
                            })
                            .execute(writer)?;

                        continue;
                    }

                    insert_or_ignore_into(container_volumes::table)
                        .values(ContainerVolume {
                            container_id: container.id,
                            volume_id,
                        })
                        .execute(writer)?;
                }

                Ok(())
            })
            .await
    }
}

impl From<CreateImage> for Image {
    fn from(value: CreateImage) -> Self {
        Self {
            id: SqlUuid::new(value.id),
            local_id: None,
            status: ImageStatus::default(),
            reference: value.reference.clone(),
            registry_auth: value.registry_auth().map(str::to_string),
        }
    }
}

impl From<CreateNetwork> for (Network, Vec<NetworkDriverOpts>) {
    fn from(
        CreateNetwork {
            id,
            driver,
            internal,
            enable_ipv6,
            options,
        }: &CreateNetwork,
    ) -> Self {
        Self {
            id: SqlUuid::new(id),
            local_id: None,
            status: NetworkStatus::default(),
            driver: driver.to_string(),
            internal: *internal,
            enable_ipv6: *enable_ipv6,
        }
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
