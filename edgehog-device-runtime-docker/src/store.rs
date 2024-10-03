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

//! Store for the state of the services.

use std::path::PathBuf;

use crate::request::{CreateImage, CreateNetwork, CreateVolume};

enum State {
    Image {
        id: Option<String>,
        req: CreateImage,
    },
    Volume {
        id: Option<String>,
        req: CreateVolume,
    },
    Network {
        id: Option<String>,
        req: CreateNetwork,
    },
    Container {
        id: Option<String>,
        req: Option<String>,
    },
    Release(),
}

#[derive(Debug, Clone)]
pub struct StateStore {
    file: PathBuf,
}

impl StateStore {
    pub fn new(file: PathBuf) -> Self {
        Self { file }
    }

    pub async fn store(&self, data: T) -> Result<(), ()> {}
}
