// This file is part of Edgehog.
//
// Copyright 2023-2024 SECO Mind Srl
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

//! Docker struct to manage containers.

use std::{
    collections::HashMap,
    fmt::{Debug, Display},
    hash::Hash,
    ops::{Deref, DerefMut},
    sync::OnceLock,
};

use bollard::{
    container::{
        Config, CreateContainerOptions, InspectContainerOptions, NetworkingConfig,
        RemoveContainerOptions, StartContainerOptions,
    },
    errors::Error as BollardError,
    models::{
        ContainerInspectResponse, EndpointSettings, HostConfig, PortBinding,
        RestartPolicy as BollardRestartPolicy,
    },
};
use tracing::{debug, info, instrument, trace, warn};
use uuid::Uuid;

use crate::{
    client::*,
    requests::{
        container::{parse_port_binding, RestartPolicy},
        BindingError,
    },
};

/// Error for the container operations.
#[non_exhaustive]
#[derive(Debug, thiserror::Error, displaydoc::Display)]
pub enum ContainerError {
    /// couldn't create the container
    Create(#[source] BollardError),
    /// couldn't inspect the container
    Inspect(#[source] BollardError),
    /// couldn't remove the container
    Remove(#[source] BollardError),
    /// couldn't start container.
    Start(#[source] BollardError),
    /// couldn't stop container
    Stop(#[source] BollardError),
    /// missing image reference in container definition
    Image,
}

/// Identifies a container univocally.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ContainerId {
    /// Id of the docker container.
    ///
    /// The id of the image is optional since it will be available only when the image is created.
    pub(crate) id: Option<String>,
    /// Assign the specified name to the container.
    ///
    /// Must match /?[a-zA-Z0-9][a-zA-Z0-9_.-]+.
    pub(crate) name: Uuid,
    /// Cache the name with a single allocation.
    ///
    /// Usually multiple functions are called in sequence.
    name_cache: OnceLock<String>,
}

impl ContainerId {
    pub(crate) fn new(id: Option<String>, name: Uuid) -> Self {
        Self {
            id,
            name,
            name_cache: OnceLock::new(),
        }
    }

    pub(crate) fn name_as_str(&self) -> &str {
        self.name_cache
            .get_or_init(|| self.name.to_string())
            .as_str()
    }

    /// Get the container id or name if it's missing.
    #[instrument(skip_all)]
    pub(crate) fn container(&self) -> &str {
        match &self.id {
            Some(id) => {
                trace!("returning id");

                id.as_str()
            }
            None => {
                trace!("id missing, returning name");

                self.name_as_str()
            }
        }
    }

    /// Set the id from docker.
    #[instrument(skip_all)]
    fn update(&mut self, id: String) {
        info!("using id {id} for container {}", self.name);

        let old_id = self.id.replace(id);

        trace!(?old_id);
    }

    pub(crate) async fn inspect(
        &mut self,
        client: &Client,
    ) -> Result<Option<ContainerInspectResponse>, ContainerError> {
        // We need to account to the case that we have an incorrect id, but it exists another
        // container with the correct name
        if let Some(id) = self.id.clone() {
            debug!("checkign the id");

            let response = self.inspect_with(client, &id).await?;

            if response.is_some() {
                return Ok(response);
            }
        }
        // Use a variable to circumvent a bug in clippy
        let name = self.name_as_str().to_string();
        self.inspect_with(client, &name).await
    }

    /// Inspect a docker container.
    ///
    /// See the [Docker API reference](https://docs.docker.com/engine/api/v1.43/#tag/Container/operation/ContainerInspect)
    #[instrument(skip_all)]
    async fn inspect_with(
        &mut self,
        client: &Client,
        name: &str,
    ) -> Result<Option<ContainerInspectResponse>, ContainerError> {
        debug!("Inspecting the {}", self);

        let res = client
            .inspect_container(name, None::<InspectContainerOptions>)
            .await;

        let container = match res {
            Ok(container) => container,
            Err(BollardError::DockerResponseServerError {
                status_code: 404,
                message,
            }) => {
                warn!("container not found: {message}");

                return Ok(None);
            }
            Err(err) => return Err(ContainerError::Inspect(err)),
        };

        trace!("container info: {container:?}");

        if let Some(id) = &container.id {
            self.update(id.clone());
        }

        Ok(Some(container))
    }

    /// Remove a docker container.
    ///
    /// See the [Docker API reference](https://docs.docker.com/engine/api/v1.43/#tag/Container/operation/ContainerDelete)
    #[instrument(skip_all)]
    pub(crate) async fn remove(&self, client: &Client) -> Result<Option<()>, ContainerError> {
        debug!("deleting {}", self);

        let opts = RemoveContainerOptions {
            v: false,
            // TODO: there is no way to force the remove from astarte
            force: false,
            link: false,
        };

        let res = client.remove_container(self.container(), Some(opts)).await;

        match res {
            Ok(()) => Ok(Some(())),
            Err(BollardError::DockerResponseServerError {
                status_code: 404,
                message,
            }) => {
                warn!("container not found: {message}");

                Ok(None)
            }
            Err(err) => return Err(ContainerError::Remove(err)),
        }
    }

    /// Start a docker container.
    ///
    /// See the [Docker API reference](https://docs.docker.com/engine/api/v1.43/#tag/Container/operation/ContainerStart)
    #[instrument(skip_all)]
    pub(crate) async fn start(&self, client: &Client) -> Result<Option<()>, ContainerError> {
        debug!("starting {self}");

        let res = client
            .start_container(self.container(), None::<StartContainerOptions<&str>>)
            .await;

        match res {
            Ok(()) => Ok(Some(())),
            Err(BollardError::DockerResponseServerError {
                status_code: 404,
                message,
            }) => {
                warn!("container not found: {message}");

                Ok(None)
            }
            Err(err) => return Err(ContainerError::Start(err)),
        }
    }

    /// Stop a docker container.
    ///
    /// See the [Docker API reference](https://docs.docker.com/engine/api/v1.43/#tag/Container/operation/ContainerStop)
    #[instrument(skip_all)]
    pub(crate) async fn stop(&self, client: &Client) -> Result<Option<()>, ContainerError> {
        debug!("stopping {self}");

        let res = client.stop_container(self.container(), None).await;

        match res {
            Ok(()) => Ok(Some(())),
            Err(BollardError::DockerResponseServerError {
                status_code: 304,
                message,
            }) => {
                debug!("container already stopped: {message}");

                Ok(Some(()))
            }
            Err(BollardError::DockerResponseServerError {
                status_code: 404,
                message,
            }) => {
                warn!("container not found: {message}");

                Ok(None)
            }
            Err(err) => return Err(ContainerError::Start(err)),
        }
    }
}

impl Display for ContainerId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(id) = &self.id {
            write!(f, "id: {id}, ")?;
        }

        write!(f, "name: {}", self.name)
    }
}

/// Docker container struct.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Container {
    pub(crate) id: ContainerId,
    /// The name (or reference) of the image to use.
    ///
    /// This should be in the form `[https://docker.io/][library/]postgres[:14]` with the fields in
    /// square brackets optional.
    pub(crate) image: String,
    /// Network mode to use for this container.
    pub(crate) network_mode: String,
    /// Network to connect the container to.
    pub(crate) networks: Vec<String>,
    /// The hostname to use for the container.
    ///
    /// Defaults to the container name.
    pub(crate) hostname: Option<String>,
    /// The behaviour to apply when the container exits.
    ///
    /// See the [create container
    /// API](https://docs.docker.com/engine/api/v1.43/#tag/Container/operation/ContainerCreate) for
    /// possible values.
    pub(crate) restart_policy: RestartPolicy,
    /// A list of environment variables to set inside the container.
    ///
    /// In the form of `NAME=VALUE`.
    pub(crate) env: Vec<String>,
    /// A list of volume bindings for this container.
    pub(crate) binds: Vec<String>,
    /// Describes the mapping of container ports to host ports.
    ///
    /// It uses the container's port-number and protocol as key in the format `<port>/<protocol>`, for
    /// example, 80/udp.
    pub(crate) port_bindings: PortBindingMap<String>,
    /// Gives the container full access to the host.
    ///
    /// Defaults to false.
    pub(crate) privileged: bool,
}

impl Container {
    /// Convert the port bindings to be used in [`HostConfig`].
    fn as_port_bindings(&self) -> HashMap<String, Option<Vec<PortBinding>>> {
        self.port_bindings
            .iter()
            .map(|(port_proto, binds)| {
                let bindings = if binds.is_empty() {
                    None
                } else {
                    Some(binds.iter().map(PortBinding::from).collect())
                };

                (port_proto.to_string(), bindings)
            })
            .collect()
    }

    /// Convert the networks into [`NetworkingConfig`]
    fn as_network_config(&self) -> HashMap<&str, EndpointSettings> {
        self.networks
            .iter()
            .map(|net_id| {
                (
                    net_id.as_ref(),
                    EndpointSettings {
                        ..Default::default()
                    },
                )
            })
            .collect()
    }

    /// Create a new docker container.
    ///
    /// See the [Docker API reference](https://docs.docker.com/engine/api/v1.43/#tag/Container/operation/ContainerCreate)
    #[instrument(skip_all)]
    pub async fn create(&mut self, client: &Client) -> Result<(), ContainerError> {
        debug!("creating the {}", self);

        let options = CreateContainerOptions::<&str>::from(&*self);
        let config = Config::<&str>::from(&*self);
        let res = client
            .create_container(Some(options), config)
            .await
            .map_err(ContainerError::Create)?;

        self.id.update(res.id);

        for warning in res.warnings {
            warn!("container created with working: {warning}");
        }

        Ok(())
    }
}

impl Display for Container {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // brackets go brrrrrr
        write!(f, "Container {{{}}}", self.id)
    }
}

impl Deref for Container {
    type Target = ContainerId;

    fn deref(&self) -> &Self::Target {
        &self.id
    }
}

impl DerefMut for Container {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.id
    }
}

impl<'a> From<&'a Container> for CreateContainerOptions<&'a str> {
    fn from(value: &'a Container) -> Self {
        CreateContainerOptions {
            name: value.name_as_str(),
            platform: None,
        }
    }
}

impl<'a> From<&'a Container> for Config<&'a str> {
    fn from(value: &'a Container) -> Self {
        let hostname = value.hostname.as_deref();
        let env = value.env.iter().map(String::as_str).collect();
        let binds = value.binds.clone();
        let port_bindings = value.as_port_bindings();
        let networks = value.as_network_config();

        let restart_policy = BollardRestartPolicy {
            name: Some(value.restart_policy.into()),
            maximum_retry_count: None,
        };

        let host_config = HostConfig {
            restart_policy: Some(restart_policy),
            binds: Some(binds),
            port_bindings: Some(port_bindings),
            privileged: Some(value.privileged),
            ..Default::default()
        };

        let networking_config = NetworkingConfig {
            endpoints_config: networks,
        };

        Config {
            hostname,
            image: Some(value.image.as_ref()),
            env: Some(env),
            host_config: Some(host_config),
            networking_config: Some(networking_config),
            ..Default::default()
        }
    }
}

fn opt_eq<T, U>(opt1: &Option<T>, opt2: &Option<U>) -> bool
where
    T: PartialEq<U>,
{
    match (opt1, opt2) {
        (None, None) => true,
        (None, Some(_)) | (Some(_), None) => false,
        (Some(v1), Some(v2)) => *v1 == *v2,
    }
}

/// Map of a port/protocol and an array of bindings.
///
/// See [`Container::port_bindings`] for more information.
#[derive(Debug, Clone, Default)]
pub struct PortBindingMap<S>(pub HashMap<String, Vec<Binding<S>>>);

impl TryFrom<&[String]> for PortBindingMap<String> {
    type Error = BindingError;

    fn try_from(value: &[String]) -> Result<Self, Self::Error> {
        value
            .iter()
            .try_fold(
                HashMap::<String, Vec<Binding<String>>>::new(),
                |mut acc, s| {
                    let bind = parse_port_binding(s)?;

                    let port_binds = acc.entry(bind.id()).or_default();

                    port_binds.push(bind.host.into());

                    Ok(acc)
                },
            )
            .map(PortBindingMap)
    }
}

impl<S> PartialEq for PortBindingMap<S>
where
    S: Eq + Hash,
{
    fn eq(&self, other: &Self) -> bool {
        self.0.eq(&other.0)
    }
}

impl<S> Eq for PortBindingMap<S> where S: Eq + Hash {}

impl<S> Deref for PortBindingMap<S>
where
    S: Hash + Eq,
{
    type Target = HashMap<String, Vec<Binding<S>>>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<S> DerefMut for PortBindingMap<S>
where
    S: Hash + Eq,
{
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

/// Represents a binding between a host IP address and a host port.
#[derive(Debug, Clone, Eq)]
pub struct Binding<S = String> {
    /// Host IP
    pub host_ip: Option<S>,
    /// Host port
    pub host_port: Option<u16>,
}

impl<S> From<&Binding<S>> for PortBinding
where
    S: AsRef<str>,
{
    fn from(value: &Binding<S>) -> Self {
        let host_ip = value.host_ip.as_ref().map(|s| s.as_ref().to_string());
        let host_port = value.host_port.map(|p| p.to_string());

        PortBinding { host_ip, host_port }
    }
}

impl<'a> From<&'a Binding<String>> for Binding<&'a str> {
    fn from(value: &'a Binding<String>) -> Self {
        Binding {
            host_ip: value.host_ip.as_deref(),
            host_port: value.host_port,
        }
    }
}

impl From<Binding<&str>> for Binding<String> {
    fn from(value: Binding<&str>) -> Self {
        Binding {
            host_ip: value.host_ip.map(ToString::to_string),
            host_port: value.host_port,
        }
    }
}

impl<S> Display for Binding<S>
where
    S: AsRef<str>,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match (&self.host_ip, self.host_port) {
            (None, None) => Ok(()),
            (Some(ip), None) => write!(f, "{}", ip.as_ref()),
            (None, Some(port)) => write!(f, "{port}"),
            (Some(ip), Some(port)) => write!(f, "{}:{port}", ip.as_ref()),
        }
    }
}

impl<S1, S2> PartialEq<Binding<S2>> for Binding<S1>
where
    S1: PartialEq<S2>,
{
    fn eq(&self, Binding { host_ip, host_port }: &Binding<S2>) -> bool {
        opt_eq(&self.host_ip, host_ip) && self.host_port.eq(host_port)
    }
}

#[cfg(test)]
mod tests {
    use mockall::predicate;

    use crate::{docker_mock, image::Image};

    use super::*;

    impl Container {
        fn new(name: Uuid, image: impl Into<String>) -> Self {
            Self {
                id: ContainerId::new(None, name),
                image: image.into(),
                hostname: None,
                restart_policy: RestartPolicy::Empty,
                env: Vec::new(),
                binds: Vec::new(),
                network_mode: "bridge".to_string(),
                networks: Vec::new(),
                port_bindings: PortBindingMap::default(),
                privileged: false,
            }
        }
    }

    #[tokio::test]
    async fn should_create() {
        let name = Uuid::now_v7();

        let docker = docker_mock!(Client::connect_with_local_defaults().unwrap(), {
            use futures::{stream, StreamExt};
            let mut mock = Client::new();
            let mut seq = mockall::Sequence::new();

            let create_res = bollard::models::ContainerCreateResponse {
                id: "id".to_string(),
                warnings: Vec::new(),
            };

            mock.expect_create_image()
                .withf(|option, _, _| {
                    option
                        .as_ref()
                        .is_some_and(|opt| opt.from_image == "hello-world:latest")
                })
                .once()
                .in_sequence(&mut seq)
                .returning(|_, _, _| stream::empty().boxed());

            mock.expect_inspect_image()
                .withf(|name| name == "hello-world:latest")
                .once()
                .in_sequence(&mut seq)
                .returning(|_| {
                    Ok(bollard::secret::ImageInspect {
                        id: Some(
                            "sha256:d2c94e258dcb3c5ac2798d32e1249e42ef01cba4841c2234249495f87264ac5a".to_string(),
                        ),
                        ..Default::default()
                    })
                });

            let name_str = name.to_string();
            mock.expect_create_container()
                .withf(move |option, config| {
                    option.as_ref().is_some_and(|opt| opt.name == name_str)
                        && config.image == Some("hello-world:latest")
                })
                .once()
                .in_sequence(&mut seq)
                .returning(move |_, _| Ok(create_res.clone()));

            mock
        });

        let mut image = Image::new(None, "hello-world:latest", None);
        image.pull(&docker).await.unwrap();

        let mut container = Container::new(name, image.reference.clone());

        container.create(&docker).await.unwrap();
    }

    #[tokio::test]
    async fn should_inspect() {
        let name = Uuid::now_v7();

        let docker = docker_mock!(Client::connect_with_local_defaults().unwrap(), {
            use futures::{stream, StreamExt};
            let mut mock = Client::new();
            let mut seq = mockall::Sequence::new();

            let create_res = bollard::models::ContainerCreateResponse {
                id: "id".to_string(),
                warnings: Vec::new(),
            };

            mock.expect_create_image()
                .withf(|option, _, _| {
                    option
                        .as_ref()
                        .is_some_and(|opt| opt.from_image == "hello-world:latest")
                })
                .once()
                .in_sequence(&mut seq)
                .returning(|_, _, _| stream::empty().boxed());

            mock.expect_inspect_image()
                .withf(|name| name == "hello-world:latest")
                .once()
                .in_sequence(&mut seq)
                .returning(|_| {
                    Ok(bollard::secret::ImageInspect {
                        id: Some(
                            "sha256:d2c94e258dcb3c5ac2798d32e1249e42ef01cba4841c2234249495f87264ac5a".to_string(),
                        ),
                        ..Default::default()
                    })
                });

            let name_cl = name.to_string();
            mock.expect_create_container()
                .withf(move |option, config| {
                    option.as_ref().is_some_and(|opt| opt.name == name_cl)
                        && config.image == Some("hello-world:latest")
                })
                .once()
                .in_sequence(&mut seq)
                .returning(move |_, _| Ok(create_res.clone()));

            let inspect_res = bollard::models::ContainerInspectResponse {
                id: Some("id".to_string()),
                name: Some(format!("/{name}")),
                image: Some("hello-world".to_string()),
                ..Default::default()
            };

            mock.expect_inspect_container()
                .withf(move |id, _option| id == "id")
                .once()
                .returning(move |_, _| Ok(inspect_res.clone()));

            mock
        });

        let mut image = Image::new(None, "hello-world:latest", None);
        image.pull(&docker).await.unwrap();

        let mut container = Container::new(name, image.reference.clone());

        container.create(&docker).await.unwrap();

        let resp = container.inspect(&docker).await.unwrap().unwrap();

        assert_eq!(resp.name, Some(format!("/{name}")));
    }

    #[tokio::test]
    async fn should_inspect_not_found() {
        let name = Uuid::now_v7();

        let docker = docker_mock!(Client::connect_with_local_defaults().unwrap(), {
            let mut mock = Client::new();

            mock.expect_inspect_container()
                .with(predicate::eq(name.to_string()), predicate::eq(None))
                .once()
                .returning(move |_, _| Err(crate::tests::not_found_response()));

            mock
        });

        let mut container = Container::new(name, "hello-world");

        let resp = container.inspect(&docker).await.unwrap();

        assert!(resp.is_none());
    }

    #[tokio::test]
    async fn should_remove() {
        let name = Uuid::now_v7();

        let docker = docker_mock!(Client::connect_with_local_defaults().unwrap(), {
            use futures::{stream, StreamExt};
            let mut mock = Client::new();
            let mut seq = mockall::Sequence::new();

            let create_res = bollard::models::ContainerCreateResponse {
                id: "id".to_string(),
                warnings: Vec::new(),
            };

            mock.expect_create_image()
                .withf(|option, _, _| {
                    option
                        .as_ref()
                        .is_some_and(|opt| opt.from_image == "hello-world:latest")
                })
                .once()
                .in_sequence(&mut seq)
                .returning(|_, _, _| stream::empty().boxed());

            mock.expect_inspect_image()
                .withf(|name| name == "hello-world:latest")
                .once()
                .in_sequence(&mut seq)
                .returning(|_| {
                    Ok(bollard::secret::ImageInspect {
                        id: Some(
                            "sha256:d2c94e258dcb3c5ac2798d32e1249e42ef01cba4841c2234249495f87264ac5a".to_string(),
                        ),
                        ..Default::default()
                    })
                });

            let name_str = name.to_string();
            mock.expect_create_container()
                .withf(move |option, config| {
                    option.as_ref().is_some_and(|opt| opt.name == name_str)
                        && config.image == Some("hello-world:latest")
                })
                .once()
                .in_sequence(&mut seq)
                .returning(move |_, _| Ok(create_res.clone()));

            mock.expect_remove_container()
                .withf(move |id, _options| id == "id")
                .once()
                .in_sequence(&mut seq)
                .returning(move |_, _| Ok(()));

            mock
        });

        let mut image = Image::new(None, "hello-world:latest", None);
        image.pull(&docker).await.unwrap();

        let mut container = Container::new(name, image.reference.clone());

        container.create(&docker).await.unwrap();

        container.remove(&docker).await.unwrap();
    }

    #[tokio::test]
    async fn should_remove_not_found() {
        let name = Uuid::now_v7();

        let docker = docker_mock!(Client::connect_with_local_defaults().unwrap(), {
            let mut mock = Client::new();

            mock.expect_remove_container()
                .with(
                    predicate::eq(name.to_string()),
                    predicate::eq(Some(RemoveContainerOptions {
                        v: false,
                        force: false,
                        link: false,
                    })),
                )
                .once()
                .returning(move |_, _| Err(crate::tests::not_found_response()));

            mock
        });

        let container = Container::new(name, "hello-world");

        container.remove(&docker).await.unwrap();
    }

    #[test]
    fn to_string_bind() {
        let cases = [
            (
                Binding {
                    host_ip: Some("127.0.0.1"),
                    host_port: Some(80),
                },
                "127.0.0.1:80",
            ),
            (
                Binding {
                    host_ip: Some("127.0.0.1"),
                    host_port: None,
                },
                "127.0.0.1",
            ),
            (
                Binding {
                    host_ip: None,
                    host_port: Some(80),
                },
                "80",
            ),
        ];

        for (case, expect) in cases {
            assert_eq!(case.to_string(), expect)
        }
    }
}
