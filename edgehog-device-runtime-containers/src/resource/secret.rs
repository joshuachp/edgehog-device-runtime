// This file is part of Edgehog.
//
// Copyright 2025, 2026 SECO Mind Srl
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

use edgehog_store::models::containers::secret::SecretStatus;

use crate::properties::secret::SecretStatus as PropertyStatus;
use crate::properties::{AvailableProp, Client, secret::AvailableSecret};

use super::{Context, Resource, Result};

#[derive(Debug, Default)]
pub(crate) struct SecretResource;

impl<D> Resource<D> for SecretResource
where
    D: Client + Send + Sync + 'static,
{
    async fn publish(ctx: &mut Context<'_, D>) -> Result<()> {
        AvailableSecret::new(&ctx.id)
            .send_status(ctx.device, PropertyStatus::Received)
            .await?;

        ctx.store
            .update_secret_status(ctx.id, SecretStatus::Published)
            .await?;

        Ok(())
    }
    async fn unset(ctx: &mut Context<'_, D>) -> Result<()> {
        AvailableSecret::new(&ctx.id).unset(ctx.device).await?;

        ctx.store.delete_secret(ctx.id).await?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use astarte_device_sdk::pairing::api::PairingApi;
    use astarte_device_sdk::store::SqliteStore;
    use astarte_device_sdk::transport::mqtt::Mqtt;
    use astarte_device_sdk_mock::MockDeviceClient;
    use edgehog_store::db;
    use mockall::Sequence;
    use tempfile::TempDir;
    use uuid::Uuid;

    use crate::Docker;
    use crate::properties::secret::tests::{
        expect_available_secret_send_status, expect_available_secret_unset,
    };
    use crate::requests::secret::tests::create_secret_req;
    use crate::store::StateStore;

    use super::*;

    #[tokio::test]
    async fn should_publish() {
        let tempdir = TempDir::new().unwrap();
        let client = Docker::connect().await.unwrap();
        let mut device = MockDeviceClient::<Mqtt<SqliteStore, PairingApi>>::new();
        let mut seq = Sequence::new();

        let db_file = tempdir.path().join("state.db");
        let handle = db::Handle::open(&db_file).await.unwrap();
        let mut store = StateStore::new(handle);

        let deployment_id = Uuid::new_v4();
        let secret = create_secret_req(deployment_id);
        let id = secret.id.0;
        store.create_secret(secret).await.unwrap();

        expect_available_secret_send_status(&mut device, &mut seq, &id, PropertyStatus::Received);

        let mut ctx = Context {
            id,
            store: &mut store,
            device: &mut device,
            client: &client,
            store_dir: tempdir.path(),
        };

        SecretResource::publish(&mut ctx).await.unwrap();
    }

    #[tokio::test]
    async fn should_unset() {
        let tempdir = TempDir::new().unwrap();
        let client = Docker::connect().await.unwrap();
        let mut device = MockDeviceClient::<Mqtt<SqliteStore, PairingApi>>::new();
        let mut seq = Sequence::new();

        let db_file = tempdir.path().join("state.db");
        let handle = db::Handle::open(&db_file).await.unwrap();
        let mut store = StateStore::new(handle);

        let deployment_id = Uuid::new_v4();
        let secret = create_secret_req(deployment_id);
        let id = secret.id.0;
        store.create_secret(secret).await.unwrap();

        expect_available_secret_unset(&mut device, &mut seq, &id);

        let mut ctx = Context {
            id,
            store: &mut store,
            device: &mut device,
            client: &client,
            store_dir: tempdir.path(),
        };

        SecretResource::unset(&mut ctx).await.unwrap();
    }
}
