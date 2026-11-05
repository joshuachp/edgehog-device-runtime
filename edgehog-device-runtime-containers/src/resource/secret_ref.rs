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

use std::fmt::Display;
use std::fs::Permissions;
use std::io;
use std::path::{Path, PathBuf};

use astarte_device_sdk::astarte_device_error::{Error, WrapError};
use edgehog_store::models::containers::secret::Secret;
use edgehog_store::models::containers::secret_ref::{SecretRef, SecretRefStatus};
use tokio::fs::File;
use tokio::io::AsyncWriteExt;
use tracing::{debug, error, instrument};
use uuid::Uuid;

use crate::properties::secret_ref::SecretRefStatus as PropertyStatus;
use crate::properties::{AvailableProp, Client, secret_ref::AvailableSecretRef};

use super::{Context, Create, Resource, ResourceError, Result, State};

const RESOURCE_NAME: &str = "secret_ref";

#[derive(Debug, Clone, Copy, PartialEq)]
enum SecretRefError {
    Io,
    #[cfg(unix)]
    Join,
}

impl Display for SecretRefError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SecretRefError::Io => write!(f, "I/O operation failed"),
            #[cfg(unix)]
            SecretRefError::Join => write!(f, "couldn't join task"),
        }
    }
}

/// Resource representing a secret mounted into a container.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SecretRefResource {
    pub(crate) secret_ref: SecretRef<'static>,
    pub(crate) secret: Secret<'static>,
    secret_ref_path: PathBuf,
}

impl SecretRefResource {
    pub(crate) fn secret_ref_dir(store_dir: &Path) -> PathBuf {
        let mut dir = store_dir.join("containers");
        dir.push("secrets");
        dir
    }

    pub(crate) fn file_path(store_dir: &Path, id: Uuid) -> PathBuf {
        Self::secret_ref_dir(store_dir).join(id.to_string())
    }

    #[cfg_attr(not(test), expect(dead_code))]
    pub(crate) fn target_path(&self) -> PathBuf {
        let target = Path::new(self.secret_ref.target.as_ref());
        if target.is_absolute() {
            target.to_path_buf()
        } else {
            Path::new("/run/secrets").join(target)
        }
    }

    #[cfg(unix)]
    fn mode(&self) -> u32 {
        match self.secret_ref.mode {
            Some(mode) => mode.0,
            None => 0o400,
        }
    }

    async fn configure_ref_file(
        &self,
        file: File,
    ) -> std::result::Result<(), Error<SecretRefError>> {
        let mut perms: Permissions;

        cfg_if::cfg_if! {
            if #[cfg(unix)] {
                use std::os::unix::fs::PermissionsExt;

                let mode =  self.mode();
                perms = Permissions::from_mode(mode);
            } else {
                let metadata = file
                    .metadata()
                    .await
                    .wrap_err_msg(SecretRefError::Io, "file metadata")?;

                perms = metadata.permissions();
            }
        };

        perms.set_readonly(true);

        file.set_permissions(perms)
            .await
            .wrap_err_msg(SecretRefError::Io, "set permissions")?;

        #[cfg(unix)]
        {
            let file = file.into_std().await;

            let uid = self.secret_ref.uid.map(|uid| uid.0);
            let gid = self.secret_ref.gid.map(|gid| gid.0);

            tokio::task::spawn_blocking(move || {
                std::os::unix::fs::fchown(&file, uid, gid)
                    .wrap_err_msg(SecretRefError::Io, "change ownership")
            })
            .await
            .wrap_err(SecretRefError::Join)??;
        }

        Ok(())
    }

    async fn write_and_configure_file(&self) -> std::result::Result<(), Error<SecretRefError>> {
        let mut opt = tokio::fs::OpenOptions::new();

        #[cfg(unix)]
        opt.mode(0o600);

        let mut file = opt
            .write(true)
            .create_new(true)
            .open(&self.secret_ref_path)
            .await
            .wrap_err_msg(SecretRefError::Io, "open file")?;

        file.write_all(self.secret.content.as_ref())
            .await
            .wrap_err_msg(SecretRefError::Io, "to write")?;

        file.flush()
            .await
            .wrap_err_msg(SecretRefError::Io, "to flush")?;

        file.sync_all()
            .await
            .wrap_err_msg(SecretRefError::Io, "sync all")?;

        self.configure_ref_file(file).await?;

        Ok(())
    }
}

impl<D> Resource<D> for SecretRefResource
where
    D: Client + Send + Sync + 'static,
{
    async fn publish(ctx: &mut Context<'_, D>) -> Result<()> {
        AvailableSecretRef::new(&ctx.id)
            .send_status(ctx.device, PropertyStatus::Received)
            .await?;

        ctx.store
            .update_secret_ref_status(ctx.id, SecretRefStatus::Published)
            .await?;

        if let Err(error) = Self::fetch(ctx).await {
            Self::on_error(ctx, &error).await;
            return Err(error);
        }

        Ok(())
    }

    async fn unset(ctx: &mut Context<'_, D>) -> Result<()> {
        AvailableSecretRef::new(&ctx.id).unset(ctx.device).await?;

        let path = Self::file_path(ctx.store_dir, ctx.id);
        if let Err(err) = tokio::fs::remove_file(&path).await
            && err.kind() != std::io::ErrorKind::NotFound
        {
            tracing::warn!(%err, ?path, "failed to delete secret ref file");
        }

        ctx.store.delete_secret_ref(ctx.id).await?;

        Ok(())
    }
}

impl<D> Create<D> for SecretRefResource
where
    D: Client + Send + Sync + 'static,
{
    const RESOURCE_NAME: &str = "secret_ref";

    #[instrument(skip_all, fields(id = %ctx.id))]
    async fn fetch(ctx: &mut Context<'_, D>) -> Result<Option<(State, Self)>> {
        let Some(secret_ref) = ctx.store.find_secret_ref(ctx.id).await? else {
            return Ok(None);
        };

        let Some(secret) = ctx.store.find_secret(secret_ref.secret_id.0).await? else {
            error!(secret_id = %secret_ref.secret_id.0, "secret not found in store");

            return Err(ResourceError::Invalid {
                id: ctx.id,
                resource: RESOURCE_NAME,
                ctx: "associated secret not found in store",
            });
        };

        let path = Self::file_path(ctx.store_dir, ctx.id);

        let this = SecretRefResource {
            secret_ref,
            secret,
            secret_ref_path: path,
        };

        let file = match tokio::fs::File::open(&this.secret_ref_path).await {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                debug!("secret ref file doesn't exists");

                AvailableSecretRef::new(&ctx.id)
                    .send_status(ctx.device, PropertyStatus::Missing)
                    .await?;

                return Ok(Some((State::Missing, this)));
            }
            Err(error) => {
                error!(%error, "couldn't open secret_ref file");

                return Err(ResourceError::Invalid {
                    id: ctx.id,
                    resource: RESOURCE_NAME,
                    ctx: "secret ref path is not a regular file",
                });
            }
        };

        this.configure_ref_file(file).await.map_err(|error| {
            error!(%error, "couldn't check ref file");

            ResourceError::Invalid {
                id: ctx.id,
                resource: RESOURCE_NAME,
                ctx: "couldn't check the file",
            }
        })?;

        AvailableSecretRef::new(&ctx.id)
            .send_status(ctx.device, PropertyStatus::Available)
            .await?;

        Ok(Some((State::Existing, this)))
    }

    #[instrument(skip_all)]
    async fn on_error(ctx: &mut Context<'_, D>, error: &(dyn std::error::Error + Send + Sync)) {
        error!(%error, "secret ref error");

        if let Err(error) = AvailableSecretRef::new(&ctx.id)
            .send_status(ctx.device, PropertyStatus::Error)
            .await
        {
            error!(%error,"couldn't set secret ref status");
        }
    }

    #[instrument(skip_all, fields(id = %ctx.id))]
    async fn create(&mut self, ctx: &mut Context<'_, D>) -> Result<()> {
        let dir = Self::secret_ref_dir(ctx.store_dir);

        tokio::fs::create_dir_all(&dir).await.map_err(|error| {
            error!(%error, path = %dir.display(), "couldn't create secret_ref directory");

            ResourceError::Invalid {
                id: ctx.id,
                resource: RESOURCE_NAME,
                ctx: "couldn't create secret_ref directory",
            }
        })?;

        self.write_and_configure_file().await.map_err(|error| {
            error!(%error, "couldn't write the secret_ref");

            ResourceError::Invalid {
                id: ctx.id,
                resource: RESOURCE_NAME,
                ctx: "couldn't set secret ref file permissions",
            }
        })?;

        AvailableSecretRef::new(&ctx.id)
            .send_status(ctx.device, PropertyStatus::Available)
            .await?;

        Ok(())
    }

    #[instrument(skip_all, fields(id = %ctx.id))]
    async fn delete(&mut self, ctx: &mut Context<'_, D>) -> Result<()> {
        let path = Self::file_path(ctx.store_dir, ctx.id);
        if let Err(err) = tokio::fs::remove_file(&path).await
            && err.kind() != std::io::ErrorKind::NotFound
        {
            tracing::warn!(%err, ?path, "failed to delete secret ref file");
            return Err(ResourceError::Invalid {
                id: ctx.id,
                resource: RESOURCE_NAME,
                ctx: "couldn't delete secret ref file",
            });
        }

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
    use crate::properties::secret_ref::tests::{
        expect_available_secret_ref_send_status, expect_available_secret_ref_unset,
    };
    use crate::requests::secret::tests::create_secret_req;
    use crate::requests::secret_ref::tests::create_secret_ref_req;
    use crate::store::StateStore;
    use crate::store::secret::tests::secret_to_store;
    use crate::store::secret_ref::tests::{insert_test_container, secret_ref_to_store};

    use super::*;

    struct TestFixture {
        tempdir: TempDir,
        client: Docker,
        device: MockDeviceClient<Mqtt<SqliteStore, PairingApi>>,
        store: StateStore,
        secret_ref_id: Uuid,
        secret_content: Vec<u8>,
    }

    async fn setup_test_fixture() -> TestFixture {
        let tempdir = TempDir::new().unwrap();

        let client = Docker::connect().await.unwrap();
        let device = MockDeviceClient::<Mqtt<SqliteStore, PairingApi>>::new();

        let db_file = tempdir.path().join("state.db");
        let handle = db::Handle::open(&db_file).await.unwrap();
        let store = StateStore::new(handle.clone());

        let deployment_id = Uuid::new_v4();
        let container_id = Uuid::new_v4();
        insert_test_container(&handle, container_id).await;

        let secret = create_secret_req(deployment_id);
        let secret_id = secret.id.0;
        let secret_content = secret.content.clone();
        store.create_secret(secret).await.unwrap();

        let secret_ref = create_secret_ref_req(container_id, deployment_id, secret_id);
        let secret_ref_id = secret_ref.id.0;
        store.create_secret_ref(secret_ref).await.unwrap();

        TestFixture {
            tempdir,
            client,
            device,
            store,
            secret_ref_id,
            secret_content,
        }
    }

    #[tokio::test]
    async fn should_publish_missing_when_file_not_found() {
        let mut fixture = setup_test_fixture().await;
        let mut seq = Sequence::new();

        expect_available_secret_ref_send_status(
            &mut fixture.device,
            &mut seq,
            &fixture.secret_ref_id,
            PropertyStatus::Received,
        );
        expect_available_secret_ref_send_status(
            &mut fixture.device,
            &mut seq,
            &fixture.secret_ref_id,
            PropertyStatus::Missing,
        );

        let mut ctx = Context {
            id: fixture.secret_ref_id,
            store: &mut fixture.store,
            device: &mut fixture.device,
            client: &fixture.client,
            store_dir: fixture.tempdir.path(),
        };

        SecretRefResource::publish(&mut ctx).await.unwrap();
    }

    #[tokio::test]
    async fn should_publish_available_when_file_matches() {
        let mut fixture = setup_test_fixture().await;
        let mut seq = Sequence::new();

        // Pre-create the secret-ref file with matching content and permissions
        let file_dir = SecretRefResource::secret_ref_dir(fixture.tempdir.path());
        tokio::fs::create_dir_all(&file_dir).await.unwrap();
        let file_path = SecretRefResource::file_path(fixture.tempdir.path(), fixture.secret_ref_id);
        tokio::fs::write(&file_path, &fixture.secret_content)
            .await
            .unwrap();

        expect_available_secret_ref_send_status(
            &mut fixture.device,
            &mut seq,
            &fixture.secret_ref_id,
            PropertyStatus::Received,
        );
        expect_available_secret_ref_send_status(
            &mut fixture.device,
            &mut seq,
            &fixture.secret_ref_id,
            PropertyStatus::Available,
        );

        let mut ctx = Context {
            id: fixture.secret_ref_id,
            store: &mut fixture.store,
            device: &mut fixture.device,
            client: &fixture.client,
            store_dir: fixture.tempdir.path(),
        };

        SecretRefResource::publish(&mut ctx).await.unwrap();
    }

    #[tokio::test]
    async fn should_create_file_exclusive_and_verify_no_write() {
        let mut fixture = setup_test_fixture().await;
        let mut seq = Sequence::new();

        // up calls fetch (which finds it missing), then creates it
        expect_available_secret_ref_send_status(
            &mut fixture.device,
            &mut seq,
            &fixture.secret_ref_id,
            PropertyStatus::Missing,
        );
        expect_available_secret_ref_send_status(
            &mut fixture.device,
            &mut seq,
            &fixture.secret_ref_id,
            PropertyStatus::Available,
        );

        let ctx = Context {
            id: fixture.secret_ref_id,
            store: &mut fixture.store,
            device: &mut fixture.device,
            client: &fixture.client,
            store_dir: fixture.tempdir.path(),
        };

        let mut resource = SecretRefResource::up(ctx).await.unwrap();

        // Verify the created file
        let file_path = SecretRefResource::file_path(fixture.tempdir.path(), fixture.secret_ref_id);
        let metadata = tokio::fs::metadata(&file_path).await.unwrap();
        assert!(metadata.is_file());

        let read_content = tokio::fs::read(&file_path).await.unwrap();
        assert_eq!(read_content, fixture.secret_content);

        // Attempting to create again with exclusive open should fail
        let mut ctx2 = Context {
            id: fixture.secret_ref_id,
            store: &mut fixture.store,
            device: &mut fixture.device,
            client: &fixture.client,
            store_dir: fixture.tempdir.path(),
        };
        let err = resource.create(&mut ctx2).await.unwrap_err();
        match err {
            ResourceError::Invalid { ctx, .. } => {
                assert_eq!(ctx, "couldn't open secret ref file with exclusive mode");
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[tokio::test]
    async fn should_error_on_content_mismatch() {
        let mut fixture = setup_test_fixture().await;

        let file_dir = SecretRefResource::secret_ref_dir(fixture.tempdir.path());
        tokio::fs::create_dir_all(&file_dir).await.unwrap();
        let file_path = SecretRefResource::file_path(fixture.tempdir.path(), fixture.secret_ref_id);
        tokio::fs::write(&file_path, b"mismatched content")
            .await
            .unwrap();

        let mut ctx = Context {
            id: fixture.secret_ref_id,
            store: &mut fixture.store,
            device: &mut fixture.device,
            client: &fixture.client,
            store_dir: fixture.tempdir.path(),
        };

        let err = SecretRefResource::fetch(&mut ctx).await.unwrap_err();
        match err {
            ResourceError::Invalid { ctx, .. } => {
                assert_eq!(ctx, "secret ref file content does not match secret content");
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[tokio::test]
    async fn should_error_on_mode_with_write_permission() {
        let mut fixture = setup_test_fixture().await;

        let file_dir = SecretRefResource::secret_ref_dir(fixture.tempdir.path());
        tokio::fs::create_dir_all(&file_dir).await.unwrap();
        let file_path = SecretRefResource::file_path(fixture.tempdir.path(), fixture.secret_ref_id);
        tokio::fs::write(&file_path, &fixture.secret_content)
            .await
            .unwrap();

        let mut ctx = Context {
            id: fixture.secret_ref_id,
            store: &mut fixture.store,
            device: &mut fixture.device,
            client: &fixture.client,
            store_dir: fixture.tempdir.path(),
        };

        let err = SecretRefResource::fetch(&mut ctx).await.unwrap_err();
        match err {
            ResourceError::Invalid { ctx, .. } => {
                assert_eq!(ctx, "secret ref file has write permissions");
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[tokio::test]
    async fn should_error_if_path_is_directory() {
        let mut fixture = setup_test_fixture().await;

        let file_path = SecretRefResource::file_path(fixture.tempdir.path(), fixture.secret_ref_id);
        tokio::fs::create_dir_all(&file_path).await.unwrap();

        let mut ctx = Context {
            id: fixture.secret_ref_id,
            store: &mut fixture.store,
            device: &mut fixture.device,
            client: &fixture.client,
            store_dir: fixture.tempdir.path(),
        };

        let err = SecretRefResource::fetch(&mut ctx).await.unwrap_err();
        match err {
            ResourceError::Invalid { ctx, .. } => {
                assert_eq!(ctx, "secret ref path is not a regular file");
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[tokio::test]
    async fn should_delete_file_on_down() {
        let mut fixture = setup_test_fixture().await;
        let mut seq = Sequence::new();

        // Pre-create the file
        let file_dir = SecretRefResource::secret_ref_dir(fixture.tempdir.path());
        tokio::fs::create_dir_all(&file_dir).await.unwrap();
        let file_path = SecretRefResource::file_path(fixture.tempdir.path(), fixture.secret_ref_id);
        tokio::fs::write(&file_path, &fixture.secret_content)
            .await
            .unwrap();

        // down calls fetch (existing), then delete, then unset
        expect_available_secret_ref_send_status(
            &mut fixture.device,
            &mut seq,
            &fixture.secret_ref_id,
            PropertyStatus::Available,
        );
        expect_available_secret_ref_unset(&mut fixture.device, &mut seq, &fixture.secret_ref_id);

        let ctx = Context {
            id: fixture.secret_ref_id,
            store: &mut fixture.store,
            device: &mut fixture.device,
            client: &fixture.client,
            store_dir: fixture.tempdir.path(),
        };

        SecretRefResource::down(ctx).await.unwrap();

        assert!(!tokio::fs::try_exists(&file_path).await.unwrap());
        assert!(
            fixture
                .store
                .find_secret_ref(fixture.secret_ref_id)
                .await
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn should_unset() {
        let mut fixture = setup_test_fixture().await;
        let mut seq = Sequence::new();

        let file_dir = SecretRefResource::secret_ref_dir(fixture.tempdir.path());
        tokio::fs::create_dir_all(&file_dir).await.unwrap();
        let file_path = SecretRefResource::file_path(fixture.tempdir.path(), fixture.secret_ref_id);
        tokio::fs::write(&file_path, &fixture.secret_content)
            .await
            .unwrap();

        expect_available_secret_ref_unset(&mut fixture.device, &mut seq, &fixture.secret_ref_id);

        let mut ctx = Context {
            id: fixture.secret_ref_id,
            store: &mut fixture.store,
            device: &mut fixture.device,
            client: &fixture.client,
            store_dir: fixture.tempdir.path(),
        };

        SecretRefResource::unset(&mut ctx).await.unwrap();

        assert!(!tokio::fs::try_exists(&file_path).await.unwrap());
        assert!(
            fixture
                .store
                .find_secret_ref(fixture.secret_ref_id)
                .await
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn should_resolve_target_path() {
        let deployment_id = Uuid::new_v4();
        let container_id = Uuid::new_v4();
        let secret = create_secret_req(deployment_id);
        let secret_store = secret_to_store(secret);
        let secret_ref = create_secret_ref_req(container_id, deployment_id, secret_store.id.0);
        let secret_ref_store = secret_ref_to_store(secret_ref);

        let resource = SecretRefResource {
            secret_ref: secret_ref_store.clone(),
            secret: secret_store.clone(),
            secret_ref_path: PathBuf::new(),
        };
        assert_eq!(
            resource.target_path(),
            PathBuf::from("/run/secrets").join(secret_ref_store.target.as_ref())
        );

        let mut abs_secret_ref =
            create_secret_ref_req(container_id, deployment_id, secret_store.id.0);
        abs_secret_ref.target = Some(crate::requests::OptString::from("/custom/path/secret"));
        let abs_secret_ref_store = secret_ref_to_store(abs_secret_ref);
        let abs_resource = SecretRefResource {
            secret_ref: abs_secret_ref_store,
            secret: secret_store,
            secret_ref_path: PathBuf::new(),
        };
        assert_eq!(
            abs_resource.target_path(),
            PathBuf::from("/custom/path/secret")
        );
    }
}
