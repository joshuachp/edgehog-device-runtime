// This file is part of Edgehog.
//
// Copyright 2026 SECO Mind Srl
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

use std::path::Path;

use async_trait::async_trait;
use edgehog_proto::config::v1::config_service_server::ConfigService;
use edgehog_proto::config::v1::{PutRequest, PutResponse};
use tonic::{Request, Response, Status};

use crate::service::EdgehogService;

#[async_trait]
impl ConfigService for EdgehogService {
    async fn put(&self, request: Request<PutRequest>) -> Result<Response<PutResponse>, Status> {
        let put = request.into_inner();

        // todo validate config

        let file_name = Path::new(&put.file_name);

        if let Some(file) = file_name.parent() {
            return Err(Status::invalid_argument("file name has a parent path"));
        }

        let Some(file_steam) = file_name.file_stem() else {
            return Err(Status::invalid_argument("missing file name"));
        };

        let mut path = self.config_dir.join(file_name);
        path.as_mut_os_string().push(".toml");

        tokio::fs::write(path, contents);

        todo!()
    }
}
