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
use edgehog_store::models::containers::secret::Secret;
use edgehog_store::models::containers::secret_ref::{
    ContainerMissingSecretRef, SecretRef, SecretRefMissingSecret, SecretRefStatus,
};
use edgehog_store::schema::containers::{
    container_secret_refs, secret_ref_missing_secrets, secret_refs,
};
use tracing::{debug, error, instrument};
use uuid::Uuid;

use crate::requests::secret_ref::CreateSecretRef;

use super::{Result, StateStore, StoreError};

impl StateStore {
    /// Stores the secret reference received from the CreateRequest
    #[instrument(skip_all, fields(%create_secret_ref.id))]
    pub(crate) async fn create_secret_ref(&self, create_secret_ref: CreateSecretRef) -> Result<()> {
        let secret_id = SqlUuid::new(create_secret_ref.secret_id);
        let mut value = SecretRef::try_from(create_secret_ref)?;

        self.handle
            .for_write(move |writer| {
                let secret_exists: bool = Secret::exists(&secret_id).get_result(writer)?;

                if !secret_exists {
                    debug!("secret is missing, storing secret_id into secret_ref_missing_secrets");

                    value.secret_id.take();
                }

                insert_or_ignore_into(secret_refs::table)
                    .values(&value)
                    .execute(writer)?;

                if !secret_exists {
                    insert_or_ignore_into(secret_ref_missing_secrets::table)
                        .values(SecretRefMissingSecret {
                            secret_ref_id: value.id,
                            secret_id,
                        })
                        .execute(writer)?;
                }

                insert_or_ignore_into(container_secret_refs::table)
                    .values(ContainerMissingSecretRef::find_by_secret_ref(&value.id))
                    .execute(writer)?;

                delete(ContainerMissingSecretRef::find_by_secret_ref(&value.id)).execute(writer)?;

                Ok(())
            })
            .await?;

        Ok(())
    }

    #[instrument(skip_all, fields(%id))]
    pub(crate) async fn find_secret_ref(&self, id: Uuid) -> Result<Option<SecretRef<'static>>> {
        let secret_ref = self
            .handle
            .for_read(move |reader| {
                SecretRef::find_id(&SqlUuid::new(id))
                    .first::<SecretRef>(reader)
                    .optional()
                    .map_err(HandleError::Query)
            })
            .await?;

        Ok(secret_ref)
    }

    /// Updates the state of a secret_ref
    #[instrument(skip(self))]
    pub(crate) async fn update_secret_ref_status(
        &self,
        secret_ref_id: Uuid,
        status: SecretRefStatus,
    ) -> Result<()> {
        self.handle
            .for_write(move |writer| {
                let updated = update(SecretRef::find_id(&SqlUuid::new(secret_ref_id)))
                    .set(secret_refs::status.eq(status))
                    .execute(writer)?;

                HandleError::check_modified(updated, 1)?;

                Ok(())
            })
            .await?;

        Ok(())
    }

    /// Deletes a [`SecretRef`]
    #[instrument(skip(self))]
    pub(crate) async fn delete_secret_ref(&self, secret_ref_id: Uuid) -> Result<()> {
        self.handle
            .for_write(move |writer| {
                let updated =
                    delete(SecretRef::find_id(&SqlUuid::new(secret_ref_id))).execute(writer)?;

                HandleError::check_modified(updated, 1)?;

                Ok(())
            })
            .await?;

        Ok(())
    }

    #[instrument(skip(self))]
    pub(crate) async fn load_secret_refs_to_publish(&self) -> Result<Vec<SqlUuid>> {
        let secret_refs = self
            .handle
            .for_read(move |reader| {
                let secret_refs = secret_refs::table
                    .select(secret_refs::id)
                    .filter(secret_refs::status.eq(SecretRefStatus::Received))
                    .load::<SqlUuid>(reader)?;

                Ok(secret_refs)
            })
            .await?;

        Ok(secret_refs)
    }
}

impl TryFrom<CreateSecretRef> for SecretRef<'static> {
    type Error = StoreError;

    fn try_from(value: CreateSecretRef) -> Result<Self> {
        let CreateSecretRef {
            id,
            deployment_id: _,
            secret_id,
            target,
            uid,
            gid,
            mode,
        } = value;

        let target = target
            .and_then(Option::<String>::from)
            .unwrap_or_else(|| secret_id.to_string());

        let mode = mode
            .map(|mode| {
                i32::try_from(mode).map_err(|err| {
                    error!(%err, mode, "invalid secret ref mode");

                    StoreError::Conversion {
                        ctx: format!("invalid secret ref mode {mode}"),
                    }
                })
            })
            .transpose()?;

        Ok(Self {
            id: SqlUuid::new(id),
            status: SecretRefStatus::default(),
            secret_id: Some(SqlUuid::new(secret_id)),
            target: target.into(),
            uid,
            gid,
            mode,
        })
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use crate::requests::secret::tests::create_secret_req;
    use crate::requests::secret_ref::tests::create_secret_ref_req;

    use super::*;

    use edgehog_store::db;
    use pretty_assertions::assert_eq;
    use tempfile::TempDir;

    pub(crate) fn secret_ref_to_store(secret_ref: CreateSecretRef) -> SecretRef<'static> {
        SecretRef::try_from(secret_ref).unwrap()
    }

    #[tokio::test]
    async fn should_store() {
        let tmp = TempDir::with_prefix("store_secret_ref").unwrap();
        let db_file = tmp.path().join("state.db");
        let db_file = db_file.to_str().unwrap();

        let handle = db::Handle::open(db_file).await.unwrap();
        let store = StateStore::new(handle);

        let deployment_id = Uuid::new_v4();
        let secret = create_secret_req(deployment_id);
        store.create_secret(secret.clone()).await.unwrap();

        let secret_ref = create_secret_ref_req(deployment_id, secret.id.0);
        store.create_secret_ref(secret_ref.clone()).await.unwrap();

        let res = store
            .find_secret_ref(secret_ref.id.0)
            .await
            .unwrap()
            .unwrap();

        let exp = secret_ref_to_store(secret_ref);

        assert_eq!(res, exp);
    }

    #[tokio::test]
    async fn should_store_missing_secret() {
        let tmp = TempDir::with_prefix("store_secret_ref_missing").unwrap();
        let db_file = tmp.path().join("state.db");
        let db_file = db_file.to_str().unwrap();

        let handle = db::Handle::open(db_file).await.unwrap();
        let store = StateStore::new(handle);

        let deployment_id = Uuid::new_v4();
        let secret = create_secret_req(deployment_id);
        let secret_ref = create_secret_ref_req(deployment_id, secret.id.0);

        store.create_secret_ref(secret_ref.clone()).await.unwrap();

        let res = store
            .find_secret_ref(secret_ref.id.0)
            .await
            .unwrap()
            .unwrap();

        let mut exp = secret_ref_to_store(secret_ref.clone());
        exp.secret_id = None;

        assert_eq!(res, exp);

        store.create_secret(secret).await.unwrap();

        let res = store
            .find_secret_ref(secret_ref.id.0)
            .await
            .unwrap()
            .unwrap();

        let exp = secret_ref_to_store(secret_ref);

        assert_eq!(res, exp);
    }

    #[tokio::test]
    async fn should_update() {
        let tmp = TempDir::with_prefix("update_secret_ref").unwrap();
        let db_file = tmp.path().join("state.db");
        let db_file = db_file.to_str().unwrap();

        let handle = db::Handle::open(db_file).await.unwrap();
        let store = StateStore::new(handle);

        let deployment_id = Uuid::new_v4();
        let secret = create_secret_req(deployment_id);
        let secret_ref = create_secret_ref_req(deployment_id, secret.id.0);
        store.create_secret(secret).await.unwrap();
        store.create_secret_ref(secret_ref.clone()).await.unwrap();

        store
            .update_secret_ref_status(secret_ref.id.0, SecretRefStatus::Published)
            .await
            .unwrap();

        let res = store
            .find_secret_ref(secret_ref.id.0)
            .await
            .unwrap()
            .unwrap();

        let mut exp = secret_ref_to_store(secret_ref);
        exp.status = SecretRefStatus::Published;

        assert_eq!(res, exp);
    }

    #[tokio::test]
    async fn should_load_for_publish() {
        let tmp = TempDir::with_prefix("load_secret_ref").unwrap();
        let db_file = tmp.path().join("state.db");
        let db_file = db_file.to_str().unwrap();

        let handle = db::Handle::open(db_file).await.unwrap();
        let store = StateStore::new(handle);

        let deployment_id = Uuid::new_v4();
        let secret_id = Uuid::new_v4();
        let secret_ref = create_secret_ref_req(deployment_id, secret_id);
        store.create_secret_ref(secret_ref.clone()).await.unwrap();

        let res = store.load_secret_refs_to_publish().await.unwrap();

        let exp = [SqlUuid::new(secret_ref.id.0)];

        assert_eq!(res, exp);
    }

    #[tokio::test]
    async fn should_not_load_published() {
        let tmp = TempDir::with_prefix("load_secret_ref_published").unwrap();
        let db_file = tmp.path().join("state.db");
        let db_file = db_file.to_str().unwrap();

        let handle = db::Handle::open(db_file).await.unwrap();
        let store = StateStore::new(handle);

        let deployment_id = Uuid::new_v4();
        let secret_id = Uuid::new_v4();
        let secret_ref = create_secret_ref_req(deployment_id, secret_id);
        store.create_secret_ref(secret_ref.clone()).await.unwrap();
        store
            .update_secret_ref_status(secret_ref.id.0, SecretRefStatus::Published)
            .await
            .unwrap();

        let res = store.load_secret_refs_to_publish().await.unwrap();

        assert!(res.is_empty());
    }

    #[tokio::test]
    async fn should_delete() {
        let tmp = TempDir::with_prefix("delete_secret_ref").unwrap();
        let db_file = tmp.path().join("state.db");
        let db_file = db_file.to_str().unwrap();

        let handle = db::Handle::open(db_file).await.unwrap();
        let store = StateStore::new(handle);

        let deployment_id = Uuid::new_v4();
        let secret_id = Uuid::new_v4();
        let secret_ref = create_secret_ref_req(deployment_id, secret_id);
        store.create_secret_ref(secret_ref.clone()).await.unwrap();

        store.delete_secret_ref(secret_ref.id.0).await.unwrap();

        let res = store.load_secret_refs_to_publish().await.unwrap();

        assert!(res.is_empty());
    }
}
