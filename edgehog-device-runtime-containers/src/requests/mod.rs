// This file is part of Edgehog.
//
// Copyright 2024-2026 SECO Mind Srl
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

//! Container requests sent from Astarte.

use std::borrow::Cow;
use std::{borrow::Borrow, fmt::Display, num::ParseIntError, ops::Deref};

use astarte_device_sdk::astarte_device_error::{Error, WrapError};
use astarte_device_sdk::{
    AstarteData, DeviceEvent, FromEvent, event::FromEventError, types::TypeError,
};
use container::CreateContainer;
use deployment::{CreateDeployment, DeploymentCommand, DeploymentUpdate};
use tracing::error;
use uuid::Uuid;

use self::device_mapping::CreateDeviceMapping;
use self::device_request::CreateDeviceRequest;
use self::env_file::CreateEnvFile;
use self::file_bind::CreateFileBind;
use self::secret::CreateSecret;
use self::secret_ref::CreateSecretRef;
use self::{image::CreateImage, network::CreateNetwork, volume::CreateVolume};

pub mod container;
pub mod deployment;
pub mod device_mapping;
pub mod device_request;
pub mod env_file;
pub mod file_bind;
pub mod image;
pub mod network;
pub mod secret;
pub mod secret_ref;
pub mod volume;

/// Error from handling the Astarte request.
#[non_exhaustive]
#[derive(Debug, displaydoc::Display, thiserror::Error)]
pub enum ReqError {
    /// couldn't parse option, expected key=value but got {0}
    Option(String),
    /// couldn't parse port binding
    PortBinding(#[from] BindingError),
}

/// Error from parsing a binding
#[non_exhaustive]
#[derive(Debug, thiserror::Error)]
pub enum BindingError {
    /// Couldn't parse binding port
    #[error("couldn't parse {binding} port {value}")]
    Port {
        /// Binding received
        binding: &'static str,
        /// Port of the binding
        value: String,
        /// Error converting the port
        #[source]
        source: ParseIntError,
    },
    /// Couldn't convert index for storage
    #[error("couldn't convert index for storage")]
    Idx,
}

/// Create request from Astarte.
#[derive(Debug, Clone, PartialEq)]
pub enum ContainerRequest {
    /// Request to create an image.
    Image(CreateImage),
    /// Request to create a volume.
    Volume(CreateVolume),
    /// Request to create a network.
    Network(CreateNetwork),
    /// Request to create a device mapping.
    DeviceMapping(CreateDeviceMapping),
    /// Request to create a device request.
    DeviceRequest(CreateDeviceRequest),
    /// Request to create a file bind.
    FileBind(CreateFileBind),
    /// Request to create an env file.
    EnvFile(CreateEnvFile),
    /// Request to create a secret.
    Secret(CreateSecret),
    /// Request to create a secret reference.
    SecretRef(CreateSecretRef),
    /// Request to create a container.
    Container(Box<CreateContainer>),
    /// Request to create a deployment.
    Deployment(CreateDeployment),
    /// Command for a deployment
    DeploymentCommand(DeploymentCommand),
    /// Update between two deployments
    DeploymentUpdate(DeploymentUpdate),
}

impl ContainerRequest {
    pub(crate) fn deployment_id(&self) -> Uuid {
        match self {
            ContainerRequest::Image(value) => value.deployment_id.0,
            ContainerRequest::Volume(value) => value.deployment_id.0,
            ContainerRequest::Network(value) => value.deployment_id.0,
            ContainerRequest::Container(value) => value.deployment_id.0,
            ContainerRequest::DeviceMapping(value) => value.deployment_id.0,
            ContainerRequest::DeviceRequest(value) => value.deployment_id.0,
            ContainerRequest::FileBind(value) => value.deployment_id.0,
            ContainerRequest::EnvFile(value) => value.deployment_id.0,
            ContainerRequest::Secret(value) => value.deployment_id.0,
            ContainerRequest::SecretRef(value) => value.deployment_id.0,
            ContainerRequest::Deployment(create_deployment) => create_deployment.id.0,
            ContainerRequest::DeploymentCommand(deployment_command) => deployment_command.id,
            ContainerRequest::DeploymentUpdate(deployment_update) => deployment_update.from,
        }
    }
}

impl FromEvent for ContainerRequest {
    type Err = Error<FromEventError>;

    fn from_event(value: DeviceEvent) -> Result<Self, Self::Err> {
        match value.interface.as_str() {
            "io.edgehog.devicemanager.apps.CreateImageRequest" => {
                CreateImage::from_event(value).map(ContainerRequest::Image)
            }
            "io.edgehog.devicemanager.apps.CreateVolumeRequest" => {
                CreateVolume::from_event(value).map(ContainerRequest::Volume)
            }
            "io.edgehog.devicemanager.apps.CreateNetworkRequest" => {
                CreateNetwork::from_event(value).map(ContainerRequest::Network)
            }
            "io.edgehog.devicemanager.apps.CreateDeviceMappingRequest" => {
                CreateDeviceMapping::from_event(value).map(ContainerRequest::DeviceMapping)
            }
            "io.edgehog.devicemanager.apps.CreateDeviceRequest" => {
                CreateDeviceRequest::from_event(value).map(ContainerRequest::DeviceRequest)
            }
            "io.edgehog.devicemanager.apps.CreateFileBindRequest" => {
                CreateFileBind::from_event(value).map(ContainerRequest::FileBind)
            }
            "io.edgehog.devicemanager.apps.CreateEnvFileRequest" => {
                CreateEnvFile::from_event(value).map(ContainerRequest::EnvFile)
            }
            "io.edgehog.devicemanager.apps.CreateSecretRequest" => {
                CreateSecret::from_event(value).map(ContainerRequest::Secret)
            }
            "io.edgehog.devicemanager.apps.CreateSecretRefRequest" => {
                CreateSecretRef::from_event(value).map(ContainerRequest::SecretRef)
            }
            "io.edgehog.devicemanager.apps.CreateContainerRequest" => {
                CreateContainer::from_event(value)
                    .map(|value| ContainerRequest::Container(Box::new(value)))
            }
            "io.edgehog.devicemanager.apps.CreateDeploymentRequest" => {
                CreateDeployment::from_event(value).map(ContainerRequest::Deployment)
            }
            "io.edgehog.devicemanager.apps.DeploymentCommand" => {
                DeploymentCommand::from_event(value).map(ContainerRequest::DeploymentCommand)
            }
            "io.edgehog.devicemanager.apps.DeploymentUpdate" => {
                DeploymentUpdate::from_event(value).map(ContainerRequest::DeploymentUpdate)
            }
            _ => Err(Error::with(
                FromEventError::Interface(
                    astarte_device_sdk::error::InterfaceError::InterfaceNotFound,
                ),
                "unrecognized interface interface",
            )
            .set_ctx(value.interface)),
        }
    }
}

/// Wrapper to convert an [`AstarteData`] to [`Uuid`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct ReqUuid(pub(crate) Uuid);

impl Display for ReqUuid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Borrow<Uuid> for ReqUuid {
    fn borrow(&self) -> &Uuid {
        &self.0
    }
}

impl Deref for ReqUuid {
    type Target = Uuid;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl TryFrom<&str> for ReqUuid {
    type Error = Error<TypeError>;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Uuid::parse_str(value).map(ReqUuid).wrap_err_with(|error| {
            error!(%error, value, "couldn't parse uuid value");

            Error::with(TypeError::Conversion, "couldn't parse uuid value")
                .set_ctx(value.to_string())
        })
    }
}

impl TryFrom<AstarteData> for ReqUuid {
    type Error = Error<TypeError>;

    fn try_from(value: AstarteData) -> Result<Self, Self::Error> {
        let value = String::try_from(value)?;

        Self::try_from(value.as_str())
    }
}

impl From<ReqUuid> for Uuid {
    fn from(value: ReqUuid) -> Self {
        value.0
    }
}

impl From<&ReqUuid> for Uuid {
    fn from(value: &ReqUuid) -> Self {
        value.0
    }
}

impl From<Uuid> for ReqUuid {
    fn from(value: Uuid) -> Self {
        ReqUuid(value)
    }
}

impl From<ReqUuid> for AstarteData {
    fn from(value: ReqUuid) -> Self {
        AstarteData::String(value.to_string())
    }
}

/// Wrapper to convert an [`AstarteData`] to [`Vec<Uuid>`].
///
/// This is required because we cannot implement [`TryFrom<AstarteData>`] for [`Vec<ReqUuid>`], because
/// of the orphan rule.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub(crate) struct VecReqUuid(pub(crate) Vec<ReqUuid>);

impl Deref for VecReqUuid {
    type Target = Vec<ReqUuid>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl TryFrom<AstarteData> for VecReqUuid {
    type Error = Error<TypeError>;

    fn try_from(value: AstarteData) -> Result<Self, Self::Error> {
        let value = Vec::<String>::try_from(value)?;

        value
            .iter()
            .map(|v| ReqUuid::try_from(v.as_str()))
            .collect::<Result<Vec<ReqUuid>, Error<TypeError>>>()
            .map(VecReqUuid)
    }
}

/// Optional non empty string
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct OptString(Option<String>);

impl TryFrom<AstarteData> for OptString {
    type Error = Error<TypeError>;

    fn try_from(value: AstarteData) -> Result<Self, Self::Error> {
        let value = String::try_from(value)?;

        Ok(Self::from(value))
    }
}

impl From<String> for OptString {
    fn from(value: String) -> Self {
        if value.is_empty() {
            OptString(None)
        } else {
            OptString(Some(value))
        }
    }
}

impl<'a> From<&'a str> for OptString {
    fn from(value: &'a str) -> Self {
        Self::from(value.to_string())
    }
}

impl From<OptString> for Option<String> {
    fn from(value: OptString) -> Self {
        value.0
    }
}

impl From<OptString> for Option<Cow<'static, str>> {
    fn from(value: OptString) -> Self {
        value.0.map(Cow::Owned)
    }
}

impl From<OptString> for AstarteData {
    fn from(value: OptString) -> Self {
        AstarteData::String(value.0.unwrap_or_default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use astarte_device_sdk::Value;
    use astarte_device_sdk::aggregate::AstarteObject;
    use astarte_device_sdk::chrono::Utc;
    use pretty_assertions::assert_eq;

    use crate::requests::ContainerRequest;
    use crate::requests::container::tests::{create_container_req, create_container_request_event};
    use crate::requests::deployment::tests::create_deployment_request_event;
    use crate::requests::deployment::{CommandValue, DeploymentCommand, DeploymentUpdate};
    use crate::requests::device_mapping::tests::{
        create_device_mapping_req, create_device_mapping_request_event,
    };
    use crate::requests::device_request::tests::{
        create_device_request, create_device_request_event,
    };
    use crate::requests::env_file::tests::{create_env_file_event, create_env_file_req};
    use crate::requests::file_bind::tests::{create_file_bind_event, create_file_bind_req};
    use crate::requests::image::tests::{create_image_req, create_image_request_event};
    use crate::requests::network::tests::{create_network_req, create_network_request_event};
    use crate::requests::secret::tests::{create_secret_event, create_secret_req};
    use crate::requests::secret_ref::tests::{create_secret_ref_event, create_secret_ref_req};
    use crate::requests::volume::tests::{create_volume_req, create_volume_request_event};

    #[test]
    fn from_event_image() {
        let deployment_id = Uuid::new_v4();

        let expected = create_image_req(deployment_id);
        let event = create_image_request_event(&expected);

        let request = ContainerRequest::from_event(event).unwrap();

        assert_eq!(request, ContainerRequest::Image(expected));
    }

    #[test]
    fn from_event_volume() {
        let deployment_id = Uuid::new_v4();

        let expect = create_volume_req(deployment_id);
        let event = create_volume_request_event(&expect);

        let request = ContainerRequest::from_event(event).unwrap();

        assert_eq!(request, ContainerRequest::Volume(expect));
    }

    #[test]
    fn from_event_network() {
        let deployment_id = Uuid::new_v4();

        let expect = create_network_req(deployment_id);
        let event = create_network_request_event(&expect);

        let request = ContainerRequest::from_event(event).unwrap();

        assert_eq!(request, ContainerRequest::Network(expect));
    }

    #[test]
    fn from_event_device_mapping() {
        let deployment_id = Uuid::new_v4();

        let expect = create_device_mapping_req(deployment_id);
        let event = create_device_mapping_request_event(&expect);

        let request = ContainerRequest::from_event(event).unwrap();

        assert_eq!(request, ContainerRequest::DeviceMapping(expect));
    }

    #[test]
    fn from_event_device_request() {
        let deployment_id = Uuid::new_v4();

        let expect = create_device_request(deployment_id);
        let event = create_device_request_event(&expect);

        let request = ContainerRequest::from_event(event).unwrap();

        assert_eq!(request, ContainerRequest::DeviceRequest(expect));
    }

    #[test]
    fn from_event_file_bind() {
        let deployment_id = Uuid::new_v4();

        let expect = create_file_bind_req(deployment_id);
        let event = create_file_bind_event(&expect);

        let request = ContainerRequest::from_event(event).unwrap();

        assert_eq!(request, ContainerRequest::FileBind(expect));
    }

    #[test]
    fn from_event_env_file() {
        let deployment_id = Uuid::new_v4();

        let expect = create_env_file_req(deployment_id);
        let event = create_env_file_event(&expect);

        let request = ContainerRequest::from_event(event).unwrap();

        assert_eq!(request, ContainerRequest::EnvFile(expect));
    }

    #[test]
    fn from_event_secret() {
        let deployment_id = Uuid::new_v4();

        let expect = create_secret_req(deployment_id);
        let event = create_secret_event(&expect);

        let request = ContainerRequest::from_event(event).unwrap();

        assert_eq!(request, ContainerRequest::Secret(expect));
    }

    #[test]
    fn from_event_secret_ref() {
        let deployment_id = Uuid::new_v4();
        let secret_id = Uuid::new_v4();
        let container_id = Uuid::new_v4();

        let expect = create_secret_ref_req(container_id, deployment_id, secret_id);
        let event = create_secret_ref_event(&expect);

        let request = ContainerRequest::from_event(event).unwrap();

        assert_eq!(request, ContainerRequest::SecretRef(expect));
    }

    #[test]
    fn from_event_container() {
        let deployment_id = Uuid::new_v4();

        let image = create_image_req(deployment_id);
        let volume = create_volume_req(deployment_id);
        let network = create_network_req(deployment_id);
        let device_mapping = create_device_mapping_req(deployment_id);
        let device_request = create_device_request(deployment_id);
        let file_bind = create_file_bind_req(deployment_id);
        let env_file = create_env_file_req(deployment_id);

        let expect = create_container_req(
            deployment_id,
            &image,
            &volume,
            &network,
            &device_mapping,
            &device_request,
            &file_bind,
            &env_file,
        );
        let event = create_container_request_event(&expect);

        let request = ContainerRequest::from_event(event).unwrap();

        assert_eq!(request, ContainerRequest::Container(Box::new(expect)));
    }

    #[test]
    fn from_event_deployment() {
        let id = ReqUuid(Uuid::new_v4());
        let containers = VecReqUuid(vec![ReqUuid(Uuid::new_v4())]);
        let event = create_deployment_request_event(&id.to_string(), &containers);

        let request = ContainerRequest::from_event(event).unwrap();

        assert_eq!(
            request,
            ContainerRequest::Deployment(CreateDeployment { id, containers })
        );
    }

    #[test]
    fn from_event_deployment_command() {
        let id = Uuid::new_v4();

        let event = DeviceEvent {
            interface: "io.edgehog.devicemanager.apps.DeploymentCommand".to_string(),
            path: format!("/{id}/command"),
            data: Value::Individual {
                data: "Start".into(),
                timestamp: Utc::now(),
            },
        };

        let request = ContainerRequest::from_event(event).unwrap();

        assert_eq!(
            request,
            ContainerRequest::DeploymentCommand(DeploymentCommand {
                id,
                command: CommandValue::Start,
            })
        );
    }

    #[test]
    fn from_event_deployment_update() {
        let from = Uuid::new_v4();
        let to = Uuid::new_v4();

        let data = AstarteObject::from_iter([
            ("from".to_string(), AstarteData::String(from.to_string())),
            ("to".to_string(), AstarteData::String(to.to_string())),
        ]);

        let event = DeviceEvent {
            interface: "io.edgehog.devicemanager.apps.DeploymentUpdate".to_string(),
            path: "/deployment".to_string(),
            data: Value::Object {
                data,
                timestamp: Utc::now(),
            },
        };

        let request = ContainerRequest::from_event(event).unwrap();

        assert_eq!(
            request,
            ContainerRequest::DeploymentUpdate(DeploymentUpdate { from, to })
        );
    }

    #[test]
    fn from_event_unrecognized_interface() {
        let event = DeviceEvent {
            interface: "io.edgehog.devicemanager.apps.UnknownInterface".to_string(),
            path: "/something".to_string(),
            data: Value::Individual {
                data: "invalid".into(),
                timestamp: Utc::now(),
            },
        };

        assert!(ContainerRequest::from_event(event).is_err());
    }

    #[test]
    fn container_request_deployment_id() {
        let deployment_id = Uuid::new_v4();
        let other_id = Uuid::new_v4();

        let image = create_image_req(deployment_id);
        let volume = create_volume_req(deployment_id);
        let network = create_network_req(deployment_id);
        let device_mapping = create_device_mapping_req(deployment_id);
        let device_request = create_device_request(deployment_id);
        let file_bind = create_file_bind_req(deployment_id);
        let env_file = create_env_file_req(deployment_id);
        let secret = create_secret_req(deployment_id);
        let container = create_container_req(
            deployment_id,
            &image,
            &volume,
            &network,
            &device_mapping,
            &device_request,
            &file_bind,
            &env_file,
        );
        let secret_ref = create_secret_ref_req(container.id.0, deployment_id, secret.id.0);
        let create_deployment = CreateDeployment {
            id: ReqUuid(deployment_id),
            containers: VecReqUuid(vec![container.id]),
        };
        let deployment_cmd = DeploymentCommand {
            id: deployment_id,
            command: CommandValue::Start,
        };
        let deployment_update = DeploymentUpdate {
            from: deployment_id,
            to: other_id,
        };

        assert_eq!(
            ContainerRequest::Image(image).deployment_id(),
            deployment_id
        );
        assert_eq!(
            ContainerRequest::Volume(volume).deployment_id(),
            deployment_id
        );
        assert_eq!(
            ContainerRequest::Network(network).deployment_id(),
            deployment_id
        );
        assert_eq!(
            ContainerRequest::DeviceMapping(device_mapping).deployment_id(),
            deployment_id
        );
        assert_eq!(
            ContainerRequest::DeviceRequest(device_request).deployment_id(),
            deployment_id
        );
        assert_eq!(
            ContainerRequest::FileBind(file_bind).deployment_id(),
            deployment_id
        );
        assert_eq!(
            ContainerRequest::EnvFile(env_file).deployment_id(),
            deployment_id
        );
        assert_eq!(
            ContainerRequest::Secret(secret).deployment_id(),
            deployment_id
        );
        assert_eq!(
            ContainerRequest::SecretRef(secret_ref).deployment_id(),
            deployment_id
        );
        assert_eq!(
            ContainerRequest::Container(Box::new(container)).deployment_id(),
            deployment_id
        );
        assert_eq!(
            ContainerRequest::Deployment(create_deployment).deployment_id(),
            deployment_id
        );
        assert_eq!(
            ContainerRequest::DeploymentCommand(deployment_cmd).deployment_id(),
            deployment_id
        );
        assert_eq!(
            ContainerRequest::DeploymentUpdate(deployment_update).deployment_id(),
            deployment_id
        );
    }

    #[test]
    fn req_uuid_conversions_and_formatting() {
        let raw = Uuid::new_v4();
        let req_uuid = ReqUuid(raw);

        // Display
        assert_eq!(req_uuid.to_string(), raw.to_string());

        // Borrow & Deref
        let borrowed: &Uuid = req_uuid.borrow();
        assert_eq!(*borrowed, raw);
        assert_eq!(*req_uuid, raw);

        // From / Into Uuid
        let from_raw: ReqUuid = raw.into();
        assert_eq!(from_raw, req_uuid);
        let into_raw: Uuid = req_uuid.into();
        assert_eq!(into_raw, raw);
        let ref_into_raw: Uuid = (&req_uuid).into();
        assert_eq!(ref_into_raw, raw);

        // TryFrom<&str>
        assert_eq!(
            ReqUuid::try_from(raw.to_string().as_str()).unwrap(),
            req_uuid
        );
        assert!(ReqUuid::try_from("not-a-uuid").is_err());

        // TryFrom<AstarteData> and From<ReqUuid> for AstarteData
        let astarte_data: AstarteData = req_uuid.into();
        assert_eq!(astarte_data, AstarteData::String(raw.to_string()));
        assert_eq!(ReqUuid::try_from(astarte_data).unwrap(), req_uuid);
        assert!(ReqUuid::try_from(AstarteData::Integer(42)).is_err());
        assert!(ReqUuid::try_from(AstarteData::String("invalid".to_string())).is_err());
    }

    #[test]
    fn vec_req_uuid_conversions() {
        let u1 = Uuid::new_v4();
        let u2 = Uuid::new_v4();
        let vec_req = VecReqUuid(vec![ReqUuid(u1), ReqUuid(u2)]);

        // Deref
        assert_eq!(vec_req.len(), 2);
        assert_eq!(vec_req[0].0, u1);
        assert_eq!(vec_req[1].0, u2);

        // TryFrom<AstarteData>
        let data = AstarteData::StringArray(vec![u1.to_string(), u2.to_string()]);
        let parsed = VecReqUuid::try_from(data).unwrap();
        assert_eq!(parsed, vec_req);

        // Errors
        assert!(VecReqUuid::try_from(AstarteData::Integer(10)).is_err());
        let bad_data = AstarteData::StringArray(vec!["not-a-uuid".to_string()]);
        assert!(VecReqUuid::try_from(bad_data).is_err());
    }

    #[test]
    fn optional_string() {
        let cases = [
            ("", OptString(None)),
            ("some", OptString(Some("some".to_string()))),
        ];

        for (case, exp) in cases {
            let res = OptString::try_from(AstarteData::from(case)).unwrap();

            assert_eq!(res, exp);
        }

        // From String & &str
        let empty_opt = OptString::from("");
        let some_opt = OptString::from("hello");
        assert_eq!(empty_opt, OptString(None));
        assert_eq!(some_opt, OptString(Some("hello".to_string())));
        assert_eq!(OptString::from(String::new()), OptString(None));
        assert_eq!(
            OptString::from("hello".to_string()),
            OptString(Some("hello".to_string()))
        );

        // Option<String> from OptString
        let s_none: Option<String> = empty_opt.clone().into();
        assert_eq!(s_none, None);
        let s_some: Option<String> = some_opt.clone().into();
        assert_eq!(s_some, Some("hello".to_string()));

        // Option<Cow<'static, str>> from OptString
        let cow_none: Option<Cow<'static, str>> = empty_opt.clone().into();
        assert_eq!(cow_none, None);
        let cow_some: Option<Cow<'static, str>> = some_opt.clone().into();
        assert_eq!(cow_some, Some(Cow::Borrowed("hello")));

        // AstarteData from OptString
        let data_none: AstarteData = empty_opt.into();
        assert_eq!(data_none, AstarteData::String("".to_string()));
        let data_some: AstarteData = some_opt.into();
        assert_eq!(data_some, AstarteData::String("hello".to_string()));

        // TryFrom error
        assert!(OptString::try_from(AstarteData::Integer(1)).is_err());
    }

    #[test]
    fn req_error_and_binding_error_display() {
        let opt_err = ReqError::Option("foo".to_string());
        assert_eq!(
            opt_err.to_string(),
            "couldn't parse option, expected key=value but got foo"
        );

        let idx_err = BindingError::Idx;
        assert_eq!(idx_err.to_string(), "couldn't convert index for storage");

        let parse_err = "abc".parse::<u16>().unwrap_err();
        let port_err = BindingError::Port {
            binding: "host",
            value: "abc".to_string(),
            source: parse_err,
        };
        assert_eq!(port_err.to_string(), "couldn't parse host port abc");

        let req_port_err = ReqError::PortBinding(BindingError::Idx);
        assert_eq!(req_port_err.to_string(), "couldn't parse port binding");
    }
}
