// This file is part of Edgehog.
//
// Copyright 2023-2024 SECO Mind Srl
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//    http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.
//
// SPDX-License-Identifier: Apache-2.0

//! Error returned when interacting with the docker daemon

use crate::{
    container::ContainerError, image::ImageError, network::NetworkError, volume::VolumeError,
};

/// Error returned form the docker daemon
#[non_exhaustive]
#[derive(Debug, displaydoc::Display, thiserror::Error)]
pub enum DockerError {
    /// couldn't connect to the docker daemon docker
    Connection(#[source] bollard::errors::Error),
    /// couldn't negotiate a supported version with the daemon docker
    Version(#[source] bollard::errors::Error),
    /// couldn't listen to events
    Envents(#[source] bollard::errors::Error),
    /// couldn't ping the docker daemon
    Ping(#[source] bollard::errors::Error),
    /// couldn't complete the image operation
    Image(#[from] ImageError),
    /// couldn't complete the volume operation
    Volume(#[from] VolumeError),
    /// couldn't complete the network operation
    Network(#[from] NetworkError),
    /// couldn't complete the container operation
    Container(#[from] ContainerError),
}
