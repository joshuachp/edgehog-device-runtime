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

use diesel::query_dsl::methods::{FilterDsl, SelectDsl};
use diesel::{
    ExpressionMethods, OptionalExtension, RunQueryDsl, delete, insert_or_ignore_into, update,
};
use edgehog_store::conversions::SqlUuid;
use edgehog_store::db::HandleError;
use edgehog_store::models::QueryModel;
use edgehog_store::models::containers::secret::{Secret, SecretStatus};
use edgehog_store::models::containers::secret_ref::SecretRefMissingSecret;
use edgehog_store::schema::containers::{secret_ref_missing_secrets, secret_refs, secrets};
use tracing::instrument;
use uuid::Uuid;

use crate::requests::secret::CreateSecret;

use super::{Result, StateStore};

impl StateStore {
    /// Stores the secret received from the CreateRequest
    #[instrument(skip_all, fields(%create_secret.id))]
    pub(crate) async fn create_secret(&self, create_secret: CreateSecret) -> Result<()> {
        let value = Secret::from(create_secret);

        self.handle
            .for_write(move |writer| {
                insert_or_ignore_into(secrets::table)
                    .values(&value)
                    .execute(writer)?;

                update(secret_refs::table)
                    .set(secret_refs::secret_id.eq(value.id))
                    .filter(
                        secret_refs::id.eq_any(
                            SecretRefMissingSecret::find_by_secret(&value.id)
                                .select(secret_ref_missing_secrets::secret_ref_id),
                        ),
                    )
                    .execute(writer)?;

                delete(SecretRefMissingSecret::find_by_secret(&value.id)).execute(writer)?;

                Ok(())
            })
            .await?;

        Ok(())
    }

    #[instrument(skip_all, fields(%id))]
    pub(crate) async fn find_secret(&self, id: Uuid) -> Result<Option<Secret<'static>>> {
        let secret = self
            .handle
            .for_read(move |reader| {
                Secret::find_id(&SqlUuid::new(id))
                    .first::<Secret>(reader)
                    .optional()
                    .map_err(HandleError::Query)
            })
            .await?;

        Ok(secret)
    }

    /// Updates the state of a secret
    #[instrument(skip(self))]
    pub(crate) async fn update_secret_status(
        &self,
        secret_id: Uuid,
        status: SecretStatus,
    ) -> Result<()> {
        self.handle
            .for_write(move |writer| {
                let updated = update(Secret::find_id(&SqlUuid::new(secret_id)))
                    .set(secrets::status.eq(status))
                    .execute(writer)?;

                HandleError::check_modified(updated, 1)?;

                Ok(())
            })
            .await?;

        Ok(())
    }

    /// Deletes a [`Secret`]
    #[instrument(skip(self))]
    pub(crate) async fn delete_secret(&self, secret_id: Uuid) -> Result<()> {
        self.handle
            .for_write(move |writer| {
                let updated = delete(Secret::find_id(&SqlUuid::new(secret_id))).execute(writer)?;

                HandleError::check_modified(updated, 1)?;

                Ok(())
            })
            .await?;

        Ok(())
    }

    #[instrument(skip(self))]
    pub(crate) async fn load_secrets_to_publish(&self) -> Result<Vec<SqlUuid>> {
        let secrets = self
            .handle
            .for_read(move |reader| {
                let secrets = secrets::table
                    .select(secrets::id)
                    .filter(secrets::status.eq(SecretStatus::Received))
                    .load::<SqlUuid>(reader)?;

                Ok(secrets)
            })
            .await?;

        Ok(secrets)
    }
}

impl From<CreateSecret> for Secret<'static> {
    fn from(
        CreateSecret {
            id,
            deployment_id: _,
            content,
        }: CreateSecret,
    ) -> Self {
        Self {
            id: SqlUuid::new(id),
            status: SecretStatus::default(),
            content: content.into(),
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use crate::requests::secret::tests::create_secret_req;

    use super::*;

    use edgehog_store::db;
    use pretty_assertions::assert_eq;
    use tempfile::TempDir;

    pub(crate) fn secret_to_store(secret: CreateSecret) -> Secret<'static> {
        Secret {
            id: secret.id.0.into(),
            status: SecretStatus::Received,
            content: secret.content.into(),
        }
    }

    #[tokio::test]
    async fn should_store() {
        let tmp = TempDir::with_prefix("store_secret").unwrap();
        let db_file = tmp.path().join("state.db");
        let db_file = db_file.to_str().unwrap();

        let handle = db::Handle::open(db_file).await.unwrap();
        let store = StateStore::new(handle);

        let deployment_id = Uuid::new_v4();
        let secret = create_secret_req(deployment_id);
        store.create_secret(secret.clone()).await.unwrap();

        let res = store.find_secret(secret.id.0).await.unwrap().unwrap();

        let exp = secret_to_store(secret);

        assert_eq!(res, exp);
    }

    #[tokio::test]
    async fn should_update() {
        let tmp = TempDir::with_prefix("update_secret").unwrap();
        let db_file = tmp.path().join("state.db");
        let db_file = db_file.to_str().unwrap();

        let handle = db::Handle::open(db_file).await.unwrap();
        let store = StateStore::new(handle);

        let deployment_id = Uuid::new_v4();
        let secret = create_secret_req(deployment_id);
        store.create_secret(secret.clone()).await.unwrap();

        store
            .update_secret_status(secret.id.0, SecretStatus::Published)
            .await
            .unwrap();

        let res = store.find_secret(secret.id.0).await.unwrap().unwrap();

        let mut exp = secret_to_store(secret);
        exp.status = SecretStatus::Published;

        assert_eq!(res, exp);
    }

    #[tokio::test]
    async fn should_load_for_publish() {
        let tmp = TempDir::with_prefix("load_secret").unwrap();
        let db_file = tmp.path().join("state.db");
        let db_file = db_file.to_str().unwrap();

        let handle = db::Handle::open(db_file).await.unwrap();
        let store = StateStore::new(handle);

        let deployment_id = Uuid::new_v4();
        let secret = create_secret_req(deployment_id);
        store.create_secret(secret.clone()).await.unwrap();

        let res = store.load_secrets_to_publish().await.unwrap();

        let exp = [SqlUuid::new(secret.id.0)];

        assert_eq!(res, exp);
    }

    #[tokio::test]
    async fn should_not_load_published() {
        let tmp = TempDir::with_prefix("load_secret_published").unwrap();
        let db_file = tmp.path().join("state.db");
        let db_file = db_file.to_str().unwrap();

        let handle = db::Handle::open(db_file).await.unwrap();
        let store = StateStore::new(handle);

        let deployment_id = Uuid::new_v4();
        let secret = create_secret_req(deployment_id);
        store.create_secret(secret.clone()).await.unwrap();
        store
            .update_secret_status(secret.id.0, SecretStatus::Published)
            .await
            .unwrap();

        let res = store.load_secrets_to_publish().await.unwrap();

        assert!(res.is_empty());
    }

    #[tokio::test]
    async fn should_delete() {
        let tmp = TempDir::with_prefix("delete_secret").unwrap();
        let db_file = tmp.path().join("state.db");
        let db_file = db_file.to_str().unwrap();

        let handle = db::Handle::open(db_file).await.unwrap();
        let store = StateStore::new(handle);

        let deployment_id = Uuid::new_v4();
        let secret = create_secret_req(deployment_id);
        store.create_secret(secret.clone()).await.unwrap();

        store.delete_secret(secret.id.0).await.unwrap();

        let res = store.load_secrets_to_publish().await.unwrap();

        assert!(res.is_empty());
    }
}
