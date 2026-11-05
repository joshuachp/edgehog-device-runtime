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

//! Secret creation request

use std::fmt::Debug;

use astarte_device_sdk::FromEvent;

use super::ReqUuid;

/// Request to create a secret to be mounted in a container.
#[derive(Clone, FromEvent, PartialEq, Eq, PartialOrd, Ord)]
#[from_event(
    interface = "io.edgehog.devicemanager.apps.CreateSecretRequest",
    path = "/secret",
    rename_all = "camelCase",
    aggregation = "object"
)]
pub struct CreateSecret {
    #[mapping(required)]
    pub(crate) id: ReqUuid,
    #[mapping(required)]
    pub(crate) deployment_id: ReqUuid,
    #[mapping(required)]
    pub(crate) content: Vec<u8>,
}

impl Debug for CreateSecret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CreateSecret")
            .field("id", &self.id)
            .field("deployment_id", &self.deployment_id)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use astarte_device_sdk::aggregate::AstarteObject;
    use astarte_device_sdk::chrono::Utc;
    use astarte_device_sdk::{AstarteData, DeviceEvent, Value};
    use pretty_assertions::assert_eq;
    use uuid::Uuid;

    use super::*;

    pub(crate) fn create_secret_req(deployment_id: Uuid) -> CreateSecret {
        CreateSecret {
            id: ReqUuid(Uuid::new_v4()),
            deployment_id: ReqUuid(deployment_id),
            content: b"secret-value".to_vec(),
        }
    }

    pub(crate) fn create_secret_event(req: &CreateSecret) -> DeviceEvent {
        let fields: AstarteObject = [
            ("id", AstarteData::String(req.id.to_string())),
            (
                "deploymentId",
                AstarteData::String(req.deployment_id.to_string()),
            ),
            ("content", AstarteData::BinaryBlob(req.content.clone())),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect();

        DeviceEvent {
            interface: "io.edgehog.devicemanager.apps.CreateSecretRequest".to_string(),
            path: "/secret".to_string(),
            data: Value::Object {
                data: fields,
                timestamp: Utc::now(),
            },
        }
    }

    #[test]
    fn should_create_secret() {
        let deployment_id = Uuid::new_v4();
        let expected = create_secret_req(deployment_id);
        let event = create_secret_event(&expected);

        let request = CreateSecret::from_event(event).unwrap();

        assert_eq!(request, expected);
    }
}
