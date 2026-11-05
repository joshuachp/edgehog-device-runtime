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

use diesel::query_dsl::methods::{FilterDsl, SelectDsl};
use diesel::{
    ExpressionMethods, OptionalExtension, RunQueryDsl, delete, insert_or_ignore_into, update,
};
use edgehog_store::conversions::{SqlU32, SqlUuid};
use edgehog_store::db::HandleError;
use edgehog_store::models::QueryModel;
use edgehog_store::models::containers::secret_ref::{SecretRef, SecretRefStatus};
use edgehog_store::schema::containers::secret_refs;
use tracing::{error, instrument};
use uuid::Uuid;

use crate::requests::secret_ref::CreateSecretRef;

use super::{Result, StateStore, StoreError};

impl StateStore {
    /// Stores the secret reference received from the CreateRequest
    #[instrument(skip_all, fields(%create_secret_ref.id))]
    pub(crate) async fn create_secret_ref(&self, create_secret_ref: CreateSecretRef) -> Result<()> {
        let value = SecretRef::try_from(create_secret_ref)?;

        self.handle
            .for_write(move |writer| {
                insert_or_ignore_into(secret_refs::table)
                    .values(&value)
                    .execute(writer)?;

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
            container_id,
            secret_id,
            target,
            uid,
            gid,
            mode,
        } = value;

        let target = target
            .and_then(Option::<String>::from)
            .unwrap_or_else(|| secret_id.to_string());

        let target_path = Path::new(&target);

        if target_path
            .file_name()
            .is_none_or(|file_name| file_name != target_path)
        {
            error!(target = %target_path.display(), "invalid secret ref target containing parent dir");

            return Err(StoreError::Conversion {
                ctx: format!("invalid secret ref target"),
            });
        }

        let uid = uid
            .map(|uid| {
                u32::try_from(uid).map(SqlU32).map_err(|err| {
                    error!(%err, mode, "invalid secret ref uid");

                    StoreError::Conversion {
                        ctx: format!("invalid secret ref uid {uid}"),
                    }
                })
            })
            .transpose()?;

        let gid = gid
            .map(|gid| {
                u32::try_from(gid).map(SqlU32).map_err(|err| {
                    error!(%err, mode, "invalid secret ref gid");

                    StoreError::Conversion {
                        ctx: format!("invalid secret ref gid {gid}"),
                    }
                })
            })
            .transpose()?;

        let mode = mode
            .map(|mode| {
                u32::try_from(mode).map(SqlU32).map_err(|err| {
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
            secret_id: SqlUuid::new(secret_id),
            container_id: SqlUuid::new(container_id),
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

    pub(crate) async fn insert_test_container(handle: &db::Handle, container_id: Uuid) {
        use edgehog_store::models::containers::container::ContainerStatus;
        use edgehog_store::schema::containers::containers;

        handle
            .for_write(move |writer| {
                diesel::insert_into(containers::table)
                    .values((
                        containers::id.eq(SqlUuid::new(container_id)),
                        containers::status.eq(ContainerStatus::default()),
                    ))
                    .execute(writer)?;

                Ok(())
            })
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn should_store() {
        let tmp = TempDir::with_prefix("store_secret_ref").unwrap();
        let db_file = tmp.path().join("state.db");
        let db_file = db_file.to_str().unwrap();

        let handle = db::Handle::open(db_file).await.unwrap();
        let store = StateStore::new(handle.clone());

        let deployment_id = Uuid::new_v4();
        let container_id = Uuid::new_v4();
        insert_test_container(&handle, container_id).await;

        let secret = create_secret_req(deployment_id);
        store.create_secret(secret.clone()).await.unwrap();

        let secret_ref = create_secret_ref_req(container_id, deployment_id, secret.id.0);
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
    async fn should_err_missing_secret() {
        let tmp = TempDir::with_prefix("store_secret_ref_missing").unwrap();
        let db_file = tmp.path().join("state.db");
        let db_file = db_file.to_str().unwrap();

        let handle = db::Handle::open(db_file).await.unwrap();
        let store = StateStore::new(handle.clone());

        let deployment_id = Uuid::new_v4();
        let container_id = Uuid::new_v4();
        insert_test_container(&handle, container_id).await;

        let secret = create_secret_req(deployment_id);
        let secret_ref = create_secret_ref_req(container_id, deployment_id, secret.id.0);

        store.create_secret_ref(secret_ref).await.unwrap_err();
    }

    #[tokio::test]
    async fn should_err_missing_container() {
        let tmp = TempDir::with_prefix("store_secret_ref_missing_container").unwrap();
        let db_file = tmp.path().join("state.db");
        let db_file = db_file.to_str().unwrap();

        let handle = db::Handle::open(db_file).await.unwrap();
        let store = StateStore::new(handle);

        let deployment_id = Uuid::new_v4();
        let secret = create_secret_req(deployment_id);
        store.create_secret(secret.clone()).await.unwrap();

        let container_id = Uuid::new_v4();
        let secret_ref = create_secret_ref_req(container_id, deployment_id, secret.id.0);

        store.create_secret_ref(secret_ref).await.unwrap_err();
    }

    #[tokio::test]
    async fn should_update() {
        let tmp = TempDir::with_prefix("update_secret_ref").unwrap();
        let db_file = tmp.path().join("state.db");
        let db_file = db_file.to_str().unwrap();

        let handle = db::Handle::open(db_file).await.unwrap();
        let store = StateStore::new(handle.clone());

        let deployment_id = Uuid::new_v4();
        let container_id = Uuid::new_v4();
        insert_test_container(&handle, container_id).await;

        let secret = create_secret_req(deployment_id);
        let secret_ref = create_secret_ref_req(container_id, deployment_id, secret.id.0);
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
        let store = StateStore::new(handle.clone());

        let deployment_id = Uuid::new_v4();
        let container_id = Uuid::new_v4();
        insert_test_container(&handle, container_id).await;

        let secret = create_secret_req(deployment_id);
        let secret_ref = create_secret_ref_req(container_id, deployment_id, secret.id.0);
        store.create_secret(secret).await.unwrap();
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
        let store = StateStore::new(handle.clone());

        let deployment_id = Uuid::new_v4();
        let container_id = Uuid::new_v4();
        insert_test_container(&handle, container_id).await;

        let secret = create_secret_req(deployment_id);
        let secret_ref = create_secret_ref_req(container_id, deployment_id, secret.id.0);
        store.create_secret(secret).await.unwrap();
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
        let store = StateStore::new(handle.clone());

        let deployment_id = Uuid::new_v4();
        let container_id = Uuid::new_v4();
        insert_test_container(&handle, container_id).await;

        let secret = create_secret_req(deployment_id);
        let secret_ref = create_secret_ref_req(container_id, deployment_id, secret.id.0);
        store.create_secret(secret).await.unwrap();
        store.create_secret_ref(secret_ref.clone()).await.unwrap();

        store.delete_secret_ref(secret_ref.id.0).await.unwrap();

        let res = store.load_secret_refs_to_publish().await.unwrap();

        assert!(res.is_empty());
    }

    #[test]
    fn should_reject_invalid_mode_and_target() {
        let deployment_id = Uuid::new_v4();
        let container_id = Uuid::new_v4();
        let secret_id = Uuid::new_v4();

        let mut neg_mode = create_secret_ref_req(container_id, deployment_id, secret_id);
        neg_mode.mode = Some(-1);
        assert!(SecretRef::try_from(neg_mode).is_err());

        let mut high_mode = create_secret_ref_req(container_id, deployment_id, secret_id);
        high_mode.mode = Some(0o1777);
        assert!(SecretRef::try_from(high_mode).is_err());

        let mut parent_dir_target = create_secret_ref_req(container_id, deployment_id, secret_id);
        parent_dir_target.target = Some(crate::requests::OptString::from("../etc/shadow"));
        assert!(SecretRef::try_from(parent_dir_target).is_err());
    }
}
