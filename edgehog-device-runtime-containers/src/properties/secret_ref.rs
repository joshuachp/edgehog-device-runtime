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

//! Available secret_ref property.

use std::fmt::Display;

use astarte_device_sdk::{AstarteData, Client};
use uuid::Uuid;

use super::AvailableProp;

const INTERFACE: &str = "io.edgehog.devicemanager.apps.AvailableSecretRefs";

/// Available secret ref property.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct AvailableSecretRef<'a> {
    id: &'a Uuid,
}

impl<'a> AvailableSecretRef<'a> {
    pub(crate) fn new(id: &'a Uuid) -> Self {
        Self { id }
    }
}

impl AvailableProp for AvailableSecretRef<'_> {
    type Data = SecretRefStatus;

    fn interface() -> &'static str {
        INTERFACE
    }

    fn id(&self) -> &Uuid {
        self.id
    }

    async fn send_status<D>(
        self,
        device: &mut D,
        data: Self::Data,
    ) -> Result<(), super::PropertyError>
    where
        D: Client + Send + Sync + 'static,
    {
        Self::send_field(self.id(), device, "status", data).await
    }

    async fn send<D>(self, device: &mut D, data: Self::Data) -> Result<(), super::PropertyError>
    where
        D: Client + Send + Sync + 'static,
    {
        Self::send_field(self.id(), device, "status", data).await
    }

    async fn unset<D>(&self, device: &mut D) -> Result<(), super::PropertyError>
    where
        D: Client + Send + Sync + 'static,
    {
        Self::unset_field(self.id(), device, "status").await
    }
}

/// Enum of the secret_ref status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SecretRefStatus {
    /// Received and stored the CreateSecretRef.
    Received,
    /// The secret_ref is missing from the Device.
    Missing,
    /// The secret_ref is available on the Device.
    Available,
    /// The secret_ref is in an error state.
    Error,
}

impl Display for SecretRefStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SecretRefStatus::Received => write!(f, "Received"),
            SecretRefStatus::Missing => write!(f, "Missing"),
            SecretRefStatus::Available => write!(f, "Available"),
            SecretRefStatus::Error => write!(f, "Error"),
        }
    }
}

impl From<SecretRefStatus> for AstarteData {
    fn from(value: SecretRefStatus) -> Self {
        AstarteData::String(value.to_string())
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use astarte_device_sdk::pairing::api::PairingApi;
    use astarte_device_sdk::store::SqliteStore;
    use astarte_device_sdk::transport::mqtt::Mqtt;
    use astarte_device_sdk_mock::{MockDeviceClient, mockall::Sequence};
    use astarte_test_utils::with_insta;
    use mockall::predicate;
    use rstest::{Context, rstest};
    use uuid::Uuid;

    use crate::properties::tests::{expect_send_field, expect_unset_field};

    use super::*;

    pub(crate) fn expect_available_secret_ref_send_status(
        client: &mut MockDeviceClient<Mqtt<SqliteStore, PairingApi>>,
        seq: &mut Sequence,
        id: &Uuid,
        status: SecretRefStatus,
    ) {
        expect_send_field(client, seq, INTERFACE, id, "status", status);
    }

    pub(crate) fn expect_available_secret_ref_unset(
        client: &mut MockDeviceClient<Mqtt<SqliteStore, PairingApi>>,
        seq: &mut Sequence,
        id: &Uuid,
    ) {
        expect_unset_field(client, seq, INTERFACE, id, "status");
    }

    #[rstest]
    #[case(SecretRefStatus::Received)]
    #[case(SecretRefStatus::Missing)]
    #[case(SecretRefStatus::Available)]
    #[case(SecretRefStatus::Error)]
    fn should_convert_status_to_string(#[case] status: SecretRefStatus, #[context] ctx: Context) {
        with_insta!({
            let name = format!("{}_{}", ctx.name, ctx.case.unwrap());

            insta::assert_snapshot!(name, status);
        });
    }

    #[rstest]
    #[case(SecretRefStatus::Received)]
    #[case(SecretRefStatus::Missing)]
    #[case(SecretRefStatus::Available)]
    #[case(SecretRefStatus::Error)]
    #[tokio::test]
    async fn should_store_secret_ref(#[case] status: SecretRefStatus) {
        let id = Uuid::new_v4();

        let secret_ref = AvailableSecretRef::new(&id);

        let mut client = MockDeviceClient::<Mqtt<SqliteStore, PairingApi>>::new();
        let mut seq = Sequence::new();

        client
            .expect_set_property()
            .once()
            .in_sequence(&mut seq)
            .with(
                predicate::eq("io.edgehog.devicemanager.apps.AvailableSecretRefs"),
                predicate::eq(format!("/{id}/status")),
                predicate::eq(AstarteData::from(status)),
            )
            .returning(|_, _, _| Ok(()));

        secret_ref.send(&mut client, status).await.unwrap();
    }

    #[tokio::test]
    async fn should_unset_secret_ref() {
        let id = Uuid::new_v4();

        let secret_ref = AvailableSecretRef::new(&id);

        let mut client = MockDeviceClient::<Mqtt<SqliteStore, PairingApi>>::new();
        let mut seq = Sequence::new();

        client
            .expect_unset_property()
            .once()
            .in_sequence(&mut seq)
            .with(
                predicate::eq("io.edgehog.devicemanager.apps.AvailableSecretRefs"),
                predicate::eq(format!("/{id}/status")),
            )
            .returning(|_, _| Ok(()));

        secret_ref.unset(&mut client).await.unwrap();
    }
}
