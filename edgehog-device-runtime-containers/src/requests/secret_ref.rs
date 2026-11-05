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

//! Secret reference creation request

use astarte_device_sdk::FromEvent;

use super::{OptString, ReqUuid};

/// Request to create a secret reference to be mounted in a container.
#[derive(Debug, Clone, FromEvent, PartialEq, Eq, PartialOrd, Ord)]
#[from_event(
    interface = "io.edgehog.devicemanager.apps.CreateSecretRefRequest",
    path = "/secretRef",
    rename_all = "camelCase",
    aggregation = "object"
)]
pub struct CreateSecretRef {
    #[mapping(required)]
    pub(crate) id: ReqUuid,
    #[mapping(required)]
    pub(crate) deployment_id: ReqUuid,
    #[mapping(required)]
    pub(crate) secret_id: ReqUuid,
    pub(crate) target: Option<OptString>,
    pub(crate) uid: Option<i64>,
    pub(crate) gid: Option<i64>,
    pub(crate) mode: Option<i64>,
}

#[cfg(test)]
pub(crate) mod tests {
    use astarte_device_sdk::aggregate::AstarteObject;
    use astarte_device_sdk::chrono::Utc;
    use astarte_device_sdk::{AstarteData, DeviceEvent, Value};
    use pretty_assertions::assert_eq;
    use uuid::Uuid;

    use super::*;

    pub(crate) fn create_secret_ref_req(deployment_id: Uuid, secret_id: Uuid) -> CreateSecretRef {
        CreateSecretRef {
            id: ReqUuid(Uuid::new_v4()),
            deployment_id: ReqUuid(deployment_id),
            secret_id: ReqUuid(secret_id),
            target: Some(OptString::from("my_secret")),
            uid: Some(1000),
            gid: Some(1000),
            mode: Some(0o444),
        }
    }

    pub(crate) fn create_secret_ref_event(req: &CreateSecretRef) -> DeviceEvent {
        let mut fields: AstarteObject = [
            ("id", AstarteData::String(req.id.to_string())),
            (
                "deploymentId",
                AstarteData::String(req.deployment_id.to_string()),
            ),
            ("secretId", AstarteData::String(req.secret_id.to_string())),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect();

        if let Some(target) = req.target.clone().and_then(Option::<String>::from) {
            fields.insert("target".to_string(), AstarteData::String(target));
        }

        if let Some(uid) = req.uid {
            fields.insert("uid".to_string(), AstarteData::LongInteger(uid));
        }

        if let Some(gid) = req.gid {
            fields.insert("gid".to_string(), AstarteData::LongInteger(gid));
        }

        if let Some(mode) = req.mode {
            fields.insert("mode".to_string(), AstarteData::LongInteger(mode));
        }

        DeviceEvent {
            interface: "io.edgehog.devicemanager.apps.CreateSecretRefRequest".to_string(),
            path: "/secretRef".to_string(),
            data: Value::Object {
                data: fields,
                timestamp: Utc::now(),
            },
        }
    }

    #[test]
    fn should_create_secret_ref() {
        let deployment_id = Uuid::new_v4();
        let secret_id = Uuid::new_v4();
        let expected = create_secret_ref_req(deployment_id, secret_id);
        let event = create_secret_ref_event(&expected);

        let request = CreateSecretRef::from_event(event).unwrap();

        assert_eq!(request, expected);
    }
}
