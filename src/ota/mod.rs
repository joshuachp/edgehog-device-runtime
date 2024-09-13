/*
 * This file is part of Edgehog.
 *
 * Copyright 2022 SECO Mind Srl
 *
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 *
 *   http://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 *
 * SPDX-License-Identifier: Apache-2.0
 */

use std::fmt::Debug;
use std::str::FromStr;
use std::{
    fmt::Display,
    path::{Path, PathBuf},
};

use async_trait::async_trait;
use futures::stream::BoxStream;
use futures::TryStreamExt;
use log::{debug, error, info, warn};
use ota_handler::{OtaInProgress, OtaMessage};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

#[cfg(test)]
use mockall::automock;

use crate::controller::actor::Actor;
use crate::controller::message::OtaRequest;
use crate::error::DeviceManagerError;
use crate::ota::rauc::BundleInfo;
use crate::repository::StateRepository;
use crate::DeviceManagerOptions;

pub(crate) mod ota_handler;
#[cfg(test)]
mod ota_handler_test;
pub(crate) mod rauc;

/// Provides deploying progress information.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DeployProgress {
    percentage: i32,
    message: String,
}

impl Display for DeployProgress {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "progress {}%: {}", self.percentage, self.message)
    }
}

/// Provides the status of the deployment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeployStatus {
    Progress(DeployProgress),
    Completed { signal: i32 },
}

/// Stream of the [`DeployStatus`] events
pub type ProgressStream = BoxStream<'static, Result<DeployStatus, DeviceManagerError>>;

/// A **trait** required for all SystemUpdate handlers that want to update a system.
#[cfg_attr(test, automock)]
#[async_trait]
pub trait SystemUpdate: Send + Sync {
    async fn install_bundle(&self, source: &str) -> Result<(), DeviceManagerError>;
    async fn last_error(&self) -> Result<String, DeviceManagerError>;
    async fn info(&self, bundle: &str) -> Result<BundleInfo, DeviceManagerError>;
    async fn operation(&self) -> Result<String, DeviceManagerError>;
    async fn compatible(&self) -> Result<String, DeviceManagerError>;
    async fn boot_slot(&self) -> Result<String, DeviceManagerError>;
    async fn receive_completed(&self) -> Result<ProgressStream, DeviceManagerError>;
    async fn get_primary(&self) -> Result<String, DeviceManagerError>;
    async fn mark(
        &self,
        state: &str,
        slot_identifier: &str,
    ) -> Result<(String, String), DeviceManagerError>;
}

/// Edgehog OTA error.
///
/// Possible errors returned by OTA.
#[derive(thiserror::Error, Debug, Clone, PartialEq)]
pub enum OtaError {
    /// Invalid OTA update request received
    #[error("InvalidRequestError: {0}")]
    Request(&'static str),
    #[error("UpdateAlreadyInProgress")]
    /// Attempted to perform OTA operation while there is another one already active*/
    UpdateAlreadyInProgress,
    #[error("NetworkError: {0}")]
    /// A generic network error occurred
    Network(String),
    #[error("IOError: {0}")]
    /// A filesystem error occurred
    Io(String),
    #[error("InternalError: {0}")]
    /// An Internal error occurred during OTA procedure
    Internal(&'static str),
    #[error("InvalidBaseImage: {0}")]
    /// Invalid OTA image received
    InvalidBaseImage(String),
    #[error("SystemRollback: {0}")]
    /// The OTA procedure boot on the wrong partition
    SystemRollback(&'static str),
    /// OTA update aborted by Edgehog half way during the procedure
    #[error("Canceled")]
    Canceled,
}

impl Default for DeployStatus {
    fn default() -> Self {
        DeployStatus::Progress(DeployProgress::default())
    }
}

const DOWNLOAD_PERC_ROUNDING_STEP: f64 = 10.0;

#[derive(Serialize, Deserialize, Debug)]
pub struct PersistentState {
    pub uuid: Uuid,
    pub slot: String,
}

#[derive(Clone, PartialEq, Debug)]
pub enum OtaStatus {
    /// The device is waiting an OTA event
    Idle,
    /// The device initializing the OTA procedure
    Init(OtaId),
    /// The device didn't has an OTA procedure pending
    NoPendingOta,
    /// The device received a valid OTA Request
    Acknowledged(OtaId),
    /// The device is in downloading process, the i32 identify the progress percentage
    Downloading(OtaId, i32),
    /// The device is in the process of deploying the update
    Deploying(OtaId, DeployProgress),
    /// The device deployed the update
    Deployed(OtaId),
    /// The device is in the process of rebooting
    Rebooting(OtaId),
    /// The device was rebooted
    Rebooted,
    /// The update procedure succeeded.
    Success(OtaId),
    /// An error happened during the update procedure.
    Error(OtaError, OtaId),
    /// The update procedure failed.
    Failure(OtaError, Option<OtaId>),
}

impl OtaStatus {
    // Checks if the OTA is cancellable
    fn is_cancellable(&self) -> bool {
        match self {
            OtaStatus::Init(_) | OtaStatus::Acknowledged(_) | OtaStatus::Downloading(_, _) => true,
            OtaStatus::Idle
            | OtaStatus::NoPendingOta
            | OtaStatus::Deploying(_, _)
            | OtaStatus::Deployed(_)
            | OtaStatus::Rebooting(_)
            | OtaStatus::Rebooted
            | OtaStatus::Success(_)
            | OtaStatus::Error(_, _)
            | OtaStatus::Failure(_, _) => false,
        }
    }
}

impl Display for OtaStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OtaStatus::Idle => write!(f, "Idle"),
            OtaStatus::Init(req) => write!(f, "Init {req}"),
            OtaStatus::NoPendingOta => write!(f, "NoPendingOta"),
            OtaStatus::Acknowledged(req) => write!(f, "Acknowledged {req}"),
            OtaStatus::Downloading(req, progress) => {
                write!(f, "Downloading {req} progress {progress}")
            }
            OtaStatus::Deploying(req, progress) => write!(f, "Deploying {req} {progress}"),
            OtaStatus::Deployed(req) => write!(f, "Deployed {req}"),
            OtaStatus::Rebooting(req) => write!(f, "Rebooting {req}"),
            OtaStatus::Rebooted => write!(f, "Rebooted"),
            OtaStatus::Success(req) => write!(f, "Success {req}"),
            OtaStatus::Error(err, req) => write!(f, "Error {req}: {err}"),
            OtaStatus::Failure(err, req) => {
                write!(f, "Failure")?;

                if let Some(req) = req {
                    write!(f, " {req}")?;
                }

                write!(f, ": {err}")
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OtaId {
    pub uuid: Uuid,
    pub url: String,
}

impl TryFrom<&OtaRequest> for OtaId {
    type Error = OtaError;

    fn try_from(value: &OtaRequest) -> Result<Self, Self::Error> {
        let uuid = Uuid::from_str(&value.uuid)
            .map_err(|_| OtaError::Request("couldn't parse the UUID"))?;

        Ok(Self {
            uuid,
            url: value.url.clone(),
        })
    }
}

impl Display for OtaId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.uuid)
    }
}

impl OtaStatus {
    pub fn ota_request(&self) -> Option<&OtaId> {
        match self {
            OtaStatus::Acknowledged(ota_request)
            | OtaStatus::Downloading(ota_request, _)
            | OtaStatus::Deploying(ota_request, _)
            | OtaStatus::Deployed(ota_request)
            | OtaStatus::Rebooting(ota_request)
            | OtaStatus::Success(ota_request)
            | OtaStatus::Error(_, ota_request) => Some(ota_request),
            OtaStatus::Failure(_, ota_request) => ota_request.as_ref(),
            _ => None,
        }
    }
}

/// Provides OTA resource accessibility only by talking with it.
pub struct Ota<T, U>
where
    T: SystemUpdate,
    U: StateRepository<PersistentState>,
{
    pub system_update: T,
    pub state_repository: U,
    pub download_file_path: PathBuf,
    pub ota_status: OtaStatus,
    pub flag: OtaInProgress,
    pub tx_publisher: mpsc::Sender<OtaStatus>,
}

#[async_trait]
impl<T, U> Actor for Ota<T, U>
where
    T: SystemUpdate,
    U: StateRepository<PersistentState>,
{
    type Msg = OtaMessage;

    fn task() -> &'static str {
        "ota"
    }

    async fn init(&mut self) -> stable_eyre::Result<()> {
        self.ota_status = OtaStatus::Rebooted;

        // Not cancellable after a reboot, this is just a placeholder
        let canel = CancellationToken::new();

        self.handle_ota_update(canel).await;

        Ok(())
    }

    async fn handle(&mut self, msg: Self::Msg) -> stable_eyre::Result<()> {
        if self.ota_status != OtaStatus::Idle {
            error!("ota request already in progress");

            return Err(OtaError::UpdateAlreadyInProgress.into());
        }

        self.ota_status = OtaStatus::Init(msg.ota_id);

        self.handle_ota_update(msg.cancel);

        Ok(())
    }
}

impl<T, U> Ota<T, U>
where
    T: SystemUpdate,
    U: StateRepository<PersistentState>,
{
    pub fn new(
        opts: &DeviceManagerOptions,
        tx_publisher: mpsc::Sender<OtaStatus>,
        flag: OtaInProgress,
        system_update: T,
        state_repository: U,
    ) -> Result<Self, DeviceManagerError> {
        Ok(Ota {
            system_update,
            state_repository,
            download_file_path: opts.download_directory.clone(),
            ota_status: OtaStatus::Rebooted,
            flag,
            tx_publisher,
        })
    }

    pub async fn last_error(&self) -> Result<String, DeviceManagerError> {
        self.system_update.last_error().await
    }

    fn get_update_file_path(&self) -> PathBuf {
        self.download_file_path.join("update.bin")
    }

    // Retries the download 5 times
    async fn retry_download(
        &self,
        url: &str,
        ota_path: &Path,
        ota_file: &str,
        req: &OtaId,
    ) -> Result<String, OtaError> {
        for i in 1..=5 {
            let res = wget(&url, &ota_path, &req.uuid, &self.tx_publisher).await;

            match res {
                Ok(()) => return Ok(ota_file.to_string()),
                Err(err) => {
                    error!("Error downloading the update: {err}");

                    if self
                        .tx_publisher
                        .send(OtaStatus::Error(err, req.clone()))
                        .await
                        .is_err()
                    {
                        warn!("ota_status_publisher dropped before send error_status")
                    }
                }
            }

            let wait = u64::pow(2, i);

            error!("Next attempt in {wait}s",);

            tokio::time::sleep(tokio::time::Duration::from_secs(wait)).await;
        }

        Err(OtaError::Internal(
            "Too many attempts to download the OTA file",
        ))
    }

    /// Handle the transition to the deploying status.
    pub async fn download(&self, ota_request: OtaId) -> OtaStatus {
        let download_file_path = self.get_update_file_path();

        let Some(download_file_str) = download_file_path.to_str() else {
            return OtaStatus::Failure(
                OtaError::Io("Wrong download file path".to_string()),
                Some(ota_request),
            );
        };

        let download_res = self
            .retry_download(
                &ota_request.url,
                &download_file_path,
                download_file_str,
                &ota_request,
            )
            .await;

        let ota_file = match download_res {
            Ok(ota_file) => ota_file,
            Err(err) => {
                return OtaStatus::Failure(err, Some(ota_request));
            }
        };

        let bundle_info = match self.system_update.info(&ota_file).await {
            Ok(info) => info,
            Err(err) => {
                let message = format!(
                    "Unable to get info from ota_file in {}",
                    download_file_path.display()
                );
                error!("{message} : {err}");

                return OtaStatus::Failure(OtaError::InvalidBaseImage(message), Some(ota_request));
            }
        };

        debug!("bundle info: {:?}", bundle_info);

        let system_image_info = match self.system_update.compatible().await {
            Ok(info) => info,
            Err(err) => {
                let message = "Unable to get info from current deployed image".to_string();

                error!("{message} : {err}");

                return OtaStatus::Failure(OtaError::InvalidBaseImage(message), Some(ota_request));
            }
        };

        if bundle_info.compatible != system_image_info {
            let message = format!(
                "bundle {} is not compatible with system {system_image_info}",
                bundle_info.compatible
            );
            error!("{message}");
            return OtaStatus::Failure(
                OtaError::InvalidBaseImage(message),
                Some(ota_request.clone()),
            );
        }

        let booted_slot = match self.system_update.boot_slot().await {
            Ok(slot) => slot,
            Err(err) => {
                let message = "Unable to identify the booted slot";

                error!("{message}: {err}");

                return OtaStatus::Failure(OtaError::Internal(message), Some(ota_request.clone()));
            }
        };

        let state = PersistentState {
            uuid: ota_request.clone().uuid,
            slot: booted_slot,
        };

        if let Err(error) = self.state_repository.write(&state).await {
            let message = "Unable to persist ota state".to_string();
            error!("{message} : {error}");
            return OtaStatus::Failure(OtaError::Io(message), Some(ota_request));
        };

        OtaStatus::Deploying(ota_request.clone(), DeployProgress::default())
    }

    /// Handle the transition to the deployed status.
    pub async fn deploy(&self, ota_request: OtaId) -> OtaStatus {
        if let Err(error) = self
            .system_update
            .install_bundle(&self.get_update_file_path().to_string_lossy())
            .await
        {
            let message = "Unable to install ota image".to_string();
            error!("{message} : {error}");
            return OtaStatus::Failure(OtaError::InvalidBaseImage(message), Some(ota_request));
        }

        if let Err(error) = self.system_update.operation().await {
            let message = "Unable to get status of ota operation";
            error!("{message} : {error}");
            return OtaStatus::Failure(OtaError::Internal(message), Some(ota_request));
        }

        let stream = self.system_update.receive_completed().await;
        let stream = match stream {
            Ok(stream) => stream,
            Err(err) => {
                let message = "Unable to get status of ota operation";
                error!("{message} : {err}");
                return OtaStatus::Failure(OtaError::Internal(message), Some(ota_request));
            }
        };

        let signal = stream
            .try_fold(None, |_, status| {
                let ota_request_cl = ota_request.clone();

                async move {
                    let progress = match status {
                        DeployStatus::Progress(progress) => progress,
                        DeployStatus::Completed { signal } => {
                            return Ok(Some(signal));
                        }
                    };

                    let res = self
                        .tx_publisher
                        .send(OtaStatus::Deploying(ota_request_cl, progress))
                        .await;

                    if let Err(err) = res {
                        error!("couldn't send progress update: {err}")
                    }

                    Ok(None)
                }
            })
            .await;

        let signal = match signal {
            Ok(Some(signal)) => signal,
            Ok(None) => {
                let message = "No progress completion event received";
                error!("{message}");
                return OtaStatus::Failure(OtaError::Internal(message), Some(ota_request));
            }
            Err(err) => {
                let message = "Unable to receive the install completed event";
                error!("{message} : {err}");
                return OtaStatus::Failure(OtaError::Internal(message), Some(ota_request));
            }
        };

        info!("Completed signal! {:?}", signal);

        match signal {
            0 => {
                info!("Update successful");

                let deployed_status = OtaStatus::Deployed(ota_request.clone());
                if self
                    .tx_publisher
                    .send(deployed_status.clone())
                    .await
                    .is_err()
                {
                    warn!("ota_status_publisher dropped before send deployed_status")
                }
                deployed_status
            }
            _ => {
                let message = format!("Update failed with signal {signal}",);
                error!("{message} : {:?}", self.last_error().await);
                OtaStatus::Failure(OtaError::InvalidBaseImage(message), Some(ota_request))
            }
        }
    }

    /// Handle the transition to rebooting status.
    pub async fn reboot(&self, ota_request: OtaId) -> OtaStatus {
        if self
            .tx_publisher
            .send(OtaStatus::Rebooting(ota_request.clone()))
            .await
            .is_err()
        {
            warn!("ota_status_publisher dropped before sending rebooting_status")
        };

        info!("Rebooting the device");

        #[cfg(not(test))]
        if let Err(error) = crate::power_management::reboot().await {
            let message = "Unable to run reboot command";
            error!("{message} : {error}");
            return OtaStatus::Failure(OtaError::Internal(message), Some(ota_request.clone()));
        }

        OtaStatus::Rebooted
    }

    /// Handle the transition to success status.
    pub async fn success(&self) -> OtaStatus {
        if !self.state_repository.exists().await {
            return OtaStatus::NoPendingOta;
        }

        info!("Found pending update");

        let ota_state = match self.state_repository.read().await {
            Ok(state) => state,
            Err(err) => {
                let message = "Unable to read pending ota state".to_string();
                error!("{message} : {}", err);
                return OtaStatus::Failure(OtaError::Io(message), None);
            }
        };

        let request_uuid = ota_state.uuid;
        let ota_request = OtaId {
            uuid: request_uuid,
            url: "".to_string(),
        };

        if let Err(error) = self.do_pending_ota(&ota_state).await {
            return OtaStatus::Failure(error, Some(ota_request));
        }

        OtaStatus::Success(ota_request)
    }

    pub async fn do_pending_ota(&self, state: &PersistentState) -> Result<(), OtaError> {
        const GOOD_STATE: &str = "good";

        let booted_slot = self.system_update.boot_slot().await.map_err(|error| {
            let message = "Unable to identify the booted slot";
            error!("{message}: {error}");
            OtaError::Internal(message)
        })?;

        if state.slot == booted_slot {
            let message = "Unable to switch slot";
            return Err(OtaError::SystemRollback(message));
        }

        let primary_slot = self.system_update.get_primary().await.map_err(|error| {
            let message = "Unable to get the current primary slot";
            error!("{message}: {error}");
            OtaError::Internal(message)
        })?;

        let (marked_slot, _) = self
            .system_update
            .mark(GOOD_STATE, &primary_slot)
            .await
            .map_err(|error| {
                let message = "Unable to run marking slot operation";
                error!("{message}: {error}");
                OtaError::Internal(message)
            })?;

        if primary_slot != marked_slot {
            let message = "Unable to mark slot";
            Err(OtaError::Internal(message))
        } else {
            Ok(())
        }
    }

    pub async fn next(&mut self) {
        self.ota_status = match self.ota_status.clone() {
            OtaStatus::Init(req) => OtaStatus::Acknowledged(req),
            OtaStatus::Acknowledged(ota_request) => OtaStatus::Downloading(ota_request, 0),
            OtaStatus::Downloading(ota_request, _) => self.download(ota_request).await,
            OtaStatus::Deploying(ota_request, _) => self.deploy(ota_request).await,
            OtaStatus::Deployed(ota_request) => self.reboot(ota_request).await,
            OtaStatus::Rebooted => self.success().await,
            OtaStatus::Error(ota_error, ota_request) => {
                OtaStatus::Failure(ota_error, Some(ota_request))
            }
            rebooting @ OtaStatus::Rebooting(_) => rebooting,
            OtaStatus::NoPendingOta | OtaStatus::Success(_) | OtaStatus::Failure(_, _) => {
                OtaStatus::Idle
            }
            OtaStatus::Idle => OtaStatus::Idle,
        };
    }

    pub async fn handle_ota_update(&mut self, cancel: CancellationToken) {
        let mut check_cancel = true;
        loop {
            self.publish_status(self.ota_status.clone()).await;

            // If the status is idle there is no OTA in progress
            if self.ota_status == OtaStatus::Idle {
                self.flag.set_in_progress(false);

                break;
            } else {
                self.flag.set_in_progress(true);
            }

            if self.ota_status.is_cancellable() {
                if cancel.run_until_cancelled(self.next()).await.is_none() {
                    check_cancel = false;
                    info!("OTA update cancelled");

                    self.ota_status = OtaStatus::Failure(OtaError::Canceled, None)
                }

                continue;
            } else if check_cancel && cancel.is_cancelled() {
                // Not cancellable
                check_cancel = false;

                self.publish_status(OtaStatus::Failure(OtaError::Canceled, None))
                    .await;
            }

            self.next().await;
        }

        self.clear().await;
    }

    async fn clear(&mut self) {
        if self.state_repository.exists().await {
            let _ = self.state_repository.clear().await.map_err(|error| {
                error!("Error clearing the state repository: {:?}", error);
            });
        }

        let path = self.get_update_file_path();
        if path.exists() {
            if let Err(e) = tokio::fs::remove_file(&path).await {
                error!("Unable to remove {}: {}", path.display(), e);
            }
        }

        self.ota_status = OtaStatus::Idle;
    }

    async fn publish_status(&self, status: OtaStatus) {
        if self.tx_publisher.send(status).await.is_err() {
            error!(
                "ota publisher disconnected before sending status {}",
                self.ota_status
            )
        }
    }
}

pub async fn wget(
    url: &str,
    file_path: &Path,
    request_uuid: &Uuid,
    ota_status_publisher: &mpsc::Sender<OtaStatus>,
) -> Result<(), OtaError> {
    use tokio_stream::StreamExt;

    if file_path.exists() {
        tokio::fs::remove_file(file_path).await.map_err(|err| {
            error!(
                "failed to remove old file '{}': {}",
                file_path.display(),
                err
            );

            OtaError::Internal("failed to remove old file")
        })?;
    }

    info!("Downloading {:?}", url);

    let result_response = reqwest::get(url).await;

    match result_response {
        Err(err) => {
            let message = "Error downloading update".to_string();
            error!("{message}: {err:?}");
            Err(OtaError::Network(message))
        }
        Ok(response) => {
            debug!("Writing {}", file_path.display());

            let total_size = response
                .content_length()
                .and_then(|size| if size == 0 { None } else { Some(size) })
                .ok_or_else(|| {
                    OtaError::Network(format!("Unable to get content length from: {url}"))
                })? as f64;

            let mut downloaded: f64 = 0.0;
            let mut last_percentage_sent = 0.0;
            let mut stream = response.bytes_stream();

            let mut os_file = tokio::fs::File::create(file_path).await.map_err(|error| {
                let message = format!("Unable to create ota_file in {file_path:?}");
                error!("{message} : {error:?}");
                OtaError::Io(message)
            })?;

            while let Some(chunk_result) = stream.next().await {
                let chunk = chunk_result.map_err(|error| {
                    let message = "Unable to parse response".to_string();
                    error!("{message} : {error:?}");
                    OtaError::Network(message)
                })?;

                if chunk.is_empty() {
                    continue;
                }

                let mut content = std::io::Cursor::new(&chunk);

                tokio::io::copy(&mut content, &mut os_file)
                    .await
                    .map_err(|error| {
                        let message = format!("Unable to write chunk to ota_file in {file_path:?}");
                        error!("{message} : {error:?}");
                        OtaError::Io(message)
                    })?;

                downloaded += chunk.len() as f64;
                let progress_percentage = (downloaded / total_size) * 100.0;
                if progress_percentage == 100.0
                    || (progress_percentage - last_percentage_sent) >= DOWNLOAD_PERC_ROUNDING_STEP
                {
                    last_percentage_sent = progress_percentage;
                    if ota_status_publisher
                        .send(OtaStatus::Downloading(
                            OtaId {
                                uuid: *request_uuid,
                                url: "".to_string(),
                            },
                            progress_percentage as i32,
                        ))
                        .await
                        .is_err()
                    {
                        warn!("ota_status_publisher dropped before send downloading_status")
                    }
                }
            }

            if total_size == downloaded {
                Ok(())
            } else {
                let message = "Unable to download file".to_string();
                error!("{message}");
                Err(OtaError::Network(message))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::io;
    use std::path::PathBuf;
    use std::time::Duration;

    use astarte_device_sdk::types::AstarteType;
    use futures::StreamExt;
    use httpmock::prelude::*;
    use tempdir::TempDir;
    use tokio::sync::mpsc;
    use uuid::Uuid;

    use crate::error::DeviceManagerError;
    use crate::ota::ota_handler_test::deploy_status_stream;
    use crate::ota::rauc::BundleInfo;
    use crate::ota::{wget, Ota, OtaId, OtaStatus, PersistentState};
    use crate::ota::{DeployProgress, DeployStatus, MockSystemUpdate, OtaError, SystemUpdate};
    use crate::repository::file_state_repository::FileStateError;
    use crate::repository::{MockStateRepository, StateRepository};

    use super::ota_handler::OtaInProgress;

    /// Creates a temporary directory that will be deleted when the returned TempDir is dropped.
    fn temp_dir(prefix: &str) -> (TempDir, PathBuf) {
        let dir = TempDir::new(&format!("edgehog-{prefix}")).unwrap();
        let path = dir.path().to_owned();

        (dir, path)
    }

    impl<T, U> Ota<T, U>
    where
        T: SystemUpdate,
        U: StateRepository<PersistentState>,
    {
        /// Create the mock with a non existent download path
        pub fn mock_new(
            system_update: T,
            state_repository: U,
            tx_publisher: mpsc::Sender<OtaStatus>,
        ) -> Self {
            Ota {
                system_update,
                state_repository,
                download_file_path: PathBuf::from("/dev/null"),
                ota_status: OtaStatus::Idle,
                tx_publisher,
                flag: OtaInProgress::default(),
            }
        }

        /// Create the mock with a usable download path
        pub fn mock_new_with_path(
            system_update: T,
            state_repository: U,
            prefix: &str,
            tx_publisher: mpsc::Sender<OtaStatus>,
        ) -> (Self, TempDir) {
            let (dir, path) = temp_dir(prefix);
            let mock = Ota {
                system_update,
                state_repository,
                download_file_path: path,
                ota_status: OtaStatus::Idle,
                tx_publisher,
                flag: OtaInProgress::default(),
            };

            (mock, dir)
        }
    }

    #[tokio::test]
    async fn last_error_ok() {
        let mut system_update = MockSystemUpdate::new();
        let state_mock = MockStateRepository::<PersistentState>::new();

        system_update
            .expect_last_error()
            .returning(|| Ok("Unable to deploy image".to_string()));

        let (tx, rx) = mpsc::channel(10);

        let ota = Ota::mock_new(system_update, state_mock, tx);

        let last_error_result = ota.last_error().await;

        assert!(last_error_result.is_ok());
        assert_eq!("Unable to deploy image", last_error_result.unwrap());
    }

    #[tokio::test]
    async fn last_error_fail() {
        let mut system_update = MockSystemUpdate::new();
        let state_mock = MockStateRepository::<PersistentState>::new();

        system_update.expect_last_error().returning(|| {
            Err(DeviceManagerError::Fatal(
                "Unable to call last error".to_string(),
            ))
        });

        let (tx, _rx) = mpsc::channel(10);

        let ota = Ota::mock_new(system_update, state_mock, tx);

        let last_error_result = ota.last_error().await;

        assert!(last_error_result.is_err());
        assert!(matches!(
            last_error_result.err().unwrap(),
            DeviceManagerError::Fatal(_)
        ))
    }

    #[tokio::test]
    async fn try_to_acknowledged_fail_empty_data() {
        let state_mock = MockStateRepository::<PersistentState>::new();
        let system_update = MockSystemUpdate::new();

        let (ota_status_publisher, mut ota_status_receiver) = mpsc::channel(1);
        let mut ota = Ota::mock_new(system_update, state_mock, ota_status_publisher);

        ota.next().await;

        let receive_result = ota_status_receiver.try_recv();
        assert!(receive_result.is_err());

        assert!(matches!(
            ota.ota_status,
            OtaStatus::Failure(OtaError::Request(_), _)
        ))
    }

    #[tokio::test]
    async fn try_to_acknowledged_fail_uuid() {
        let state_mock = MockStateRepository::<PersistentState>::new();
        let system_update = MockSystemUpdate::new();

        let data = HashMap::from([(
            "url".to_string(),
            AstarteType::String("http://instance.ota.bin".to_string()),
        )]);

        let (ota_status_publisher, mut ota_status_receiver) = mpsc::channel(1);
        let mut ota = Ota::mock_new(system_update, state_mock, ota_status_publisher);

        ota.next().await;

        let receive_result = ota_status_receiver.try_recv();
        assert!(receive_result.is_err());

        assert!(matches!(
            ota.ota_status,
            OtaStatus::Failure(OtaError::Request(_), _)
        ))
    }

    #[tokio::test]
    async fn try_to_acknowledged_fail_data_with_one_key() {
        let state_mock = MockStateRepository::<PersistentState>::new();
        let system_update = MockSystemUpdate::new();

        let data = HashMap::from([
            (
                "url".to_string(),
                AstarteType::String("http://instance.ota.bin".to_string()),
            ),
            (
                "uuid".to_string(),
                AstarteType::String("bad_uuid".to_string()),
            ),
            (
                "operation".to_string(),
                AstarteType::String("Update".to_string()),
            ),
        ]);

        let (ota_status_publisher, mut ota_status_receiver) = mpsc::channel(1);
        let mut ota = Ota::mock_new(system_update, state_mock, ota_status_publisher);

        ota.next().await;

        let receive_result = ota_status_receiver.try_recv();
        assert!(receive_result.is_err());

        assert!(matches!(
            ota.ota_status,
            OtaStatus::Failure(OtaError::Request(_), _)
        ))
    }

    #[tokio::test]
    async fn ota_event_fail_data_with_wrong_astarte_type() {
        let system_update = MockSystemUpdate::new();
        let state_mock = MockStateRepository::<PersistentState>::new();

        let mut data = HashMap::new();
        data.insert(
            "url".to_owned(),
            AstarteType::String("http://ota.bin".to_owned()),
        );
        data.insert("uuid".to_owned(), AstarteType::Integer(0));
        data.insert(
            "operation".to_string(),
            AstarteType::String("Update".to_string()),
        );

        let (ota_status_publisher, mut ota_status_receiver) = mpsc::channel(1);
        let mut ota = Ota::mock_new(system_update, state_mock, ota_status_publisher);
        ota.next().await;

        let receive_result = ota_status_receiver.try_recv();
        assert!(receive_result.is_err());

        assert!(matches!(
            ota.ota_status,
            OtaStatus::Failure(OtaError::Request(_), _)
        ))
    }

    #[tokio::test]
    async fn try_to_acknowledged_success() {
        let state_mock = MockStateRepository::<PersistentState>::new();
        let system_update = MockSystemUpdate::new();

        let uuid = Uuid::new_v4();

        let (ota_status_publisher, mut ota_status_receiver) = mpsc::channel(1);
        let mut ota = Ota::mock_new(system_update, state_mock, ota_status_publisher);
        ota.ota_status = OtaStatus::Init(OtaId {
            url: "http://instance.ota.bin".to_string(),
            uuid,
        });

        ota.next().await;

        let receive_result = ota_status_receiver.try_recv();
        assert!(receive_result.is_ok());
        let ota_status_received = receive_result.unwrap();
        assert!(matches!(ota_status_received, OtaStatus::Acknowledged(_)));

        let receive_result = ota_status_receiver.try_recv();
        assert!(receive_result.is_err());

        assert!(matches!(ota.ota_status, OtaStatus::Acknowledged(_)))
    }

    #[tokio::test]
    async fn try_to_downloading_success() {
        let state_mock = MockStateRepository::<PersistentState>::new();
        let system_update = MockSystemUpdate::new();
        let ota_request = OtaId::default();

        let (ota_status_publisher, mut ota_status_receiver) = mpsc::channel(1);
        let mut ota = Ota::mock_new(system_update, state_mock, ota_status_publisher);

        ota.next().await;

        let receive_result = ota_status_receiver.try_recv();
        assert!(receive_result.is_ok());
        let ota_status_received = receive_result.unwrap();
        assert!(matches!(ota_status_received, OtaStatus::Downloading(_, 0)));

        let receive_result = ota_status_receiver.try_recv();
        assert!(receive_result.is_err());

        assert!(matches!(ota.ota_status, OtaStatus::Downloading(_, _)));
    }

    #[tokio::test]
    async fn try_to_deploying_fail_ota_request() {
        let state_mock = MockStateRepository::<PersistentState>::new();
        let mut system_update = MockSystemUpdate::new();

        system_update
            .expect_info()
            .returning(|_: &str| Err(DeviceManagerError::Fatal("Unable to get info".to_string())));

        let mut ota_request = OtaId::default();
        let binary_content = b"\x80\x02\x03";
        let binary_size = binary_content.len();

        let server = MockServer::start_async().await;
        ota_request.url = server.url("/ota.bin");
        let mock_ota_file_request = server
            .mock_async(|when, then| {
                when.method(GET).path("/ota.bin");
                then.status(200)
                    .header("content-Length", binary_size.to_string())
                    .body(binary_content);
            })
            .await;

        let (ota_status_publisher, mut ota_status_receiver) = mpsc::channel(1);
        let (ota, _dir) = Ota::mock_new_with_path(
            system_update,
            state_mock,
            "fail_ota_request",
            ota_status_publisher,
        );

        let ota_status = ota.download(ota_request).await;
        mock_ota_file_request.assert_async().await;

        let receive_result = ota_status_receiver.try_recv();
        assert!(receive_result.is_ok());
        let ota_status_received = receive_result.unwrap();
        assert!(matches!(
            ota_status_received,
            OtaStatus::Downloading(_, 100)
        ));

        let receive_result = ota_status_receiver.try_recv();
        assert!(receive_result.is_err());

        assert!(matches!(
            ota_status,
            OtaStatus::Failure(OtaError::InvalidBaseImage(_), _),
        ));
    }

    #[tokio::test]
    async fn try_to_deploying_fail_5_wget() {
        let state_mock = MockStateRepository::<PersistentState>::new();
        let mut system_update = MockSystemUpdate::new();

        system_update.expect_info().returning(|_: &str| {
            Ok(BundleInfo {
                compatible: "rauc-demo-x86".to_string(),
                version: "1".to_string(),
            })
        });

        let mut ota_request = OtaId::default();

        let server = MockServer::start_async().await;
        ota_request.url = server.url("/ota.bin");
        let mock_ota_file_request = server
            .mock_async(|when, then| {
                when.method(GET).path("/ota.bin");
                then.status(404);
            })
            .await;

        let (ota_status_publisher, mut ota_status_receiver) = mpsc::channel(4);
        let (ota, _dir) = Ota::mock_new_with_path(
            system_update,
            state_mock,
            "fail_5_wget",
            ota_status_publisher,
        );

        tokio::time::pause();

        let ota_status = tokio::spawn(async move { ota.download(ota_request).await });

        tokio::time::advance(tokio::time::Duration::from_secs(60)).await;

        let ota_status = ota_status.await.expect("join error");

        for _ in 0..4 {
            let receive_result = ota_status_receiver.try_recv();
            assert!(receive_result.is_ok());
            let ota_status_received = receive_result.unwrap();
            assert!(matches!(
                ota_status_received,
                OtaStatus::Error(OtaError::Network(_), _)
            ));
        }

        let receive_result = ota_status_receiver.try_recv();
        assert!(receive_result.is_err());

        assert!(matches!(
            ota_status,
            OtaStatus::Failure(OtaError::Network(_), _)
        ));

        mock_ota_file_request.assert_hits_async(5).await;
    }

    #[tokio::test]
    async fn try_to_deploying_fail_ota_info() {
        let state_mock = MockStateRepository::<PersistentState>::new();
        let mut system_update = MockSystemUpdate::new();

        system_update
            .expect_info()
            .returning(|_: &str| Err(DeviceManagerError::Fatal("Unable to get info".to_string())));

        let mut ota_request = OtaId::default();
        let binary_content = b"\x80\x02\x03";
        let binary_size = binary_content.len();

        let server = MockServer::start_async().await;
        ota_request.url = server.url("/ota.bin");
        let mock_ota_file_request = server
            .mock_async(|when, then| {
                when.method(GET).path("/ota.bin");
                then.status(200)
                    .header("content-Length", binary_size.to_string())
                    .body(binary_content);
            })
            .await;

        let (ota_status_publisher, mut ota_status_receiver) = mpsc::channel(1);
        let (ota, _dir) = Ota::mock_new_with_path(
            system_update,
            state_mock,
            "fail_ota_info",
            ota_status_publisher,
        );

        let ota_status = ota.download(ota_request).await;
        mock_ota_file_request.assert_async().await;

        let receive_result = ota_status_receiver.try_recv();
        assert!(receive_result.is_ok());
        let ota_status_received = receive_result.unwrap();
        assert!(matches!(
            ota_status_received,
            OtaStatus::Downloading(_, 100)
        ));

        assert!(matches!(
            ota_status,
            OtaStatus::Failure(OtaError::InvalidBaseImage(_), _),
        ));
    }

    #[tokio::test]
    async fn try_to_deploying_fail_ota_call_compatible() {
        let state_mock = MockStateRepository::<PersistentState>::new();
        let mut system_update = MockSystemUpdate::new();

        system_update.expect_info().returning(|_: &str| {
            Ok(BundleInfo {
                compatible: "rauc-demo-x86".to_string(),
                version: "1".to_string(),
            })
        });

        system_update
            .expect_compatible()
            .returning(|| Err(DeviceManagerError::Fatal("empty value".to_string())));

        let binary_content = b"\x80\x02\x03";
        let binary_size = binary_content.len();

        let mut ota_request = OtaId::default();
        let server = MockServer::start_async().await;
        ota_request.url = server.url("/ota.bin");
        let mock_ota_file_request = server
            .mock_async(|when, then| {
                when.method(GET).path("/ota.bin");
                then.status(200)
                    .header("content-Length", binary_size.to_string())
                    .body(binary_content);
            })
            .await;

        let (ota_status_publisher, mut ota_status_receiver) = mpsc::channel(1);
        let (ota, _dir) = Ota::mock_new_with_path(
            system_update,
            state_mock,
            "fail_ota_call_compatible",
            ota_status_publisher,
        );

        let ota_status = ota.download(ota_request).await;
        mock_ota_file_request.assert_async().await;

        let receive_result = ota_status_receiver.try_recv();
        assert!(receive_result.is_ok());
        let ota_status_received = receive_result.unwrap();
        assert!(matches!(
            ota_status_received,
            OtaStatus::Downloading(_, 100)
        ));

        let receive_result = ota_status_receiver.try_recv();
        assert!(receive_result.is_err());

        assert!(matches!(
            ota_status,
            OtaStatus::Failure(OtaError::InvalidBaseImage(_), _)
        ));
    }

    #[tokio::test]
    async fn try_to_deploying_fail_compatible() {
        let state_mock = MockStateRepository::<PersistentState>::new();
        let mut system_update = MockSystemUpdate::new();

        system_update.expect_info().returning(|_: &str| {
            Ok(BundleInfo {
                compatible: "rauc-demo-x86".to_string(),
                version: "1".to_string(),
            })
        });

        system_update
            .expect_compatible()
            .returning(|| Ok("rauc-demo-arm".to_string()));

        let mut ota_request = OtaId::default();

        let binary_content = b"\x80\x02\x03";
        let binary_size = binary_content.len();

        let server = MockServer::start_async().await;
        ota_request.url = server.url("/ota.bin");
        let mock_ota_file_request = server
            .mock_async(|when, then| {
                when.method(GET).path("/ota.bin");
                then.status(200)
                    .header("content-Length", binary_size.to_string())
                    .body(binary_content);
            })
            .await;

        let (ota_status_publisher, mut ota_status_receiver) = mpsc::channel(1);
        let (ota, _dir) = Ota::mock_new_with_path(
            system_update,
            state_mock,
            "fail_compatible",
            ota_status_publisher,
        );

        let ota_status = ota.download(ota_request).await;
        mock_ota_file_request.assert_async().await;

        let receive_result = ota_status_receiver.try_recv();
        assert!(receive_result.is_ok());
        let ota_status_received = receive_result.unwrap();
        assert!(matches!(
            ota_status_received,
            OtaStatus::Downloading(_, 100)
        ));

        let receive_result = ota_status_receiver.try_recv();
        assert!(receive_result.is_err());

        assert!(matches!(
            ota_status,
            OtaStatus::Failure(OtaError::InvalidBaseImage(_), _)
        ));
    }

    #[tokio::test]
    async fn try_to_deploying_fail_call_boot_slot() {
        let state_mock = MockStateRepository::<PersistentState>::new();
        let mut system_update = MockSystemUpdate::new();

        system_update.expect_info().returning(|_: &str| {
            Ok(BundleInfo {
                compatible: "rauc-demo-x86".to_string(),
                version: "1".to_string(),
            })
        });

        system_update
            .expect_compatible()
            .returning(|| Ok("rauc-demo-x86".to_string()));

        system_update.expect_boot_slot().returning(|| {
            Err(DeviceManagerError::Fatal(
                "unable to call boot slot".to_string(),
            ))
        });

        let binary_content = b"\x80\x02\x03";
        let binary_size = binary_content.len();

        let mut ota_request = OtaId::default();
        let server = MockServer::start_async().await;
        let ota_url = server.url("/ota.bin");
        ota_request.url = ota_url;
        let mock_ota_file_request = server
            .mock_async(|when, then| {
                when.method(GET).path("/ota.bin");
                then.status(200)
                    .header("content-Length", binary_size.to_string())
                    .body(binary_content);
            })
            .await;

        let (ota_status_publisher, mut ota_status_receiver) = mpsc::channel(1);
        let (ota, _dir) = Ota::mock_new_with_path(
            system_update,
            state_mock,
            "fail_call_boot_slot",
            ota_status_publisher,
        );

        let ota_status = ota.download(ota_request).await;
        mock_ota_file_request.assert_async().await;

        let receive_result = ota_status_receiver.try_recv();
        assert!(receive_result.is_ok());
        let ota_status_received = receive_result.unwrap();
        assert!(matches!(
            ota_status_received,
            OtaStatus::Downloading(_, 100)
        ));

        let receive_result = ota_status_receiver.try_recv();
        assert!(receive_result.is_err());

        assert!(matches!(
            ota_status,
            OtaStatus::Failure(OtaError::Internal(_), _)
        ));
    }

    #[tokio::test]
    async fn try_to_deploying_fail_write_state() {
        let mut state_mock = MockStateRepository::<PersistentState>::new();
        let mut system_update = MockSystemUpdate::new();

        state_mock.expect_write().returning(|_| {
            Err(FileStateError::Write {
                path: "/ota.bin".into(),
                backtrace: io::Error::new(io::ErrorKind::PermissionDenied, "permission denied"),
            })
        });

        system_update.expect_info().returning(|_: &str| {
            Ok(BundleInfo {
                compatible: "rauc-demo-x86".to_string(),
                version: "1".to_string(),
            })
        });

        system_update
            .expect_compatible()
            .returning(|| Ok("rauc-demo-x86".to_string()));

        system_update
            .expect_boot_slot()
            .returning(|| Ok("A".to_string()));

        let binary_content = b"\x80\x02\x03";
        let binary_size = binary_content.len();
        let mut ota_request = OtaId::default();
        let server = MockServer::start_async().await;
        let ota_url = server.url("/ota.bin");
        ota_request.url = ota_url;

        let mock_ota_file_request = server
            .mock_async(|when, then| {
                when.method(GET).path("/ota.bin");
                then.status(200)
                    .header("content-Length", binary_size.to_string())
                    .body(binary_content);
            })
            .await;

        tokio::time::pause();

        let (ota_status_publisher, mut ota_status_receiver) = mpsc::channel(10);
        let (ota, _) = Ota::mock_new_with_path(
            system_update,
            state_mock,
            "fail_write_state",
            ota_status_publisher,
        );

        let ota_status = ota.download(ota_request).await;

        tokio::time::advance(Duration::from_secs(60)).await;

        let receive_result = ota_status_receiver.try_recv();
        assert!(receive_result.is_ok());
        assert!(matches!(ota_status, OtaStatus::Failure(OtaError::Io(_), _)));

        mock_ota_file_request.assert_hits_async(5).await;
    }

    #[tokio::test]
    async fn try_to_deploying_success() {
        let mut state_mock = MockStateRepository::<PersistentState>::new();
        state_mock.expect_write().returning(|_| Ok(()));

        let mut system_update = MockSystemUpdate::new();

        system_update.expect_info().returning(|_: &str| {
            Ok(BundleInfo {
                compatible: "rauc-demo-x86".to_string(),
                version: "1".to_string(),
            })
        });

        system_update
            .expect_compatible()
            .returning(|| Ok("rauc-demo-x86".to_string()));

        system_update
            .expect_boot_slot()
            .returning(|| Ok("A".to_string()));

        let mut ota_request = OtaId::default();
        let binary_content = b"\x80\x02\x03";
        let binary_size = binary_content.len();

        let server = MockServer::start_async().await;
        let ota_url = server.url("/ota.bin");
        ota_request.url = ota_url;
        let mock_ota_file_request = server
            .mock_async(|when, then| {
                when.method(GET).path("/ota.bin");
                then.status(200)
                    .header("content-Length", binary_size.to_string())
                    .body(binary_content);
            })
            .await;

        let (ota_status_publisher, mut ota_status_receiver) = mpsc::channel(2);
        let (ota, _dir) = Ota::mock_new_with_path(
            system_update,
            state_mock,
            "deploying_success",
            ota_status_publisher,
        );

        let ota_status = ota.download(ota_request).await;
        mock_ota_file_request.assert_async().await;

        let receive_result = ota_status_receiver.try_recv();
        assert!(receive_result.is_ok());
        let ota_status_received = receive_result.unwrap();
        assert!(matches!(
            ota_status_received,
            OtaStatus::Downloading(_, 100)
        ));

        let receive_result = ota_status_receiver.try_recv();
        assert!(receive_result.is_ok());
        let ota_status_received = receive_result.unwrap();
        assert!(matches!(ota_status_received, OtaStatus::Deploying(_, _)));

        let receive_result = ota_status_receiver.try_recv();
        assert!(receive_result.is_err());

        assert!(matches!(ota_status, OtaStatus::Deploying(_, _)));
    }

    #[tokio::test]
    async fn try_to_deployed_fail_install_bundle() {
        let state_mock = MockStateRepository::<PersistentState>::new();
        let mut system_update = MockSystemUpdate::new();

        system_update
            .expect_install_bundle()
            .returning(|_| Err(DeviceManagerError::Fatal("install fail".to_string())));

        let (ota_status_publisher, mut ota_status_receiver) = mpsc::channel(1);
        let (ota, _dir) = Ota::mock_new_with_path(
            system_update,
            state_mock,
            "fail_install_bundle",
            ota_status_publisher,
        );

        let ota_status = ota.deploy(OtaId::default()).await;

        let receive_result = ota_status_receiver.try_recv();
        assert!(receive_result.is_err());

        assert!(matches!(
            ota_status,
            OtaStatus::Failure(OtaError::InvalidBaseImage(_), _)
        ));
    }

    #[tokio::test]
    async fn try_to_deployed_fail_operation() {
        let state_mock = MockStateRepository::<PersistentState>::new();
        let mut system_update = MockSystemUpdate::new();

        system_update.expect_install_bundle().returning(|_| Ok(()));
        system_update
            .expect_operation()
            .returning(|| Err(DeviceManagerError::Fatal("operation call fail".to_string())));

        let (ota_status_publisher, mut ota_status_receiver) = mpsc::channel(1);
        let (ota, _dir) = Ota::mock_new_with_path(
            system_update,
            state_mock,
            "deployed_fail_operation",
            ota_status_publisher,
        );

        let ota_status = ota.deploy(OtaId::default()).await;

        let receive_result = ota_status_receiver.try_recv();
        assert!(receive_result.is_err());

        assert!(matches!(
            ota_status,
            OtaStatus::Failure(OtaError::Internal(_), _)
        ));
    }

    #[tokio::test]
    async fn try_to_deployed_fail_receive_completed() {
        let state_mock = MockStateRepository::<PersistentState>::new();
        let mut system_update = MockSystemUpdate::new();

        system_update.expect_install_bundle().returning(|_| Ok(()));
        system_update
            .expect_operation()
            .returning(|| Ok("".to_string()));
        system_update.expect_receive_completed().returning(|| {
            Err(DeviceManagerError::Fatal(
                "receive_completed call fail".to_string(),
            ))
        });

        let (ota_status_publisher, mut ota_status_receiver) = mpsc::channel(1);
        let (ota, _dir) = Ota::mock_new_with_path(
            system_update,
            state_mock,
            "deployed_fail_receive_completed",
            ota_status_publisher,
        );

        let ota_status = ota.deploy(OtaId::default()).await;

        let receive_result = ota_status_receiver.try_recv();
        assert!(receive_result.is_err());

        assert!(matches!(
            ota_status,
            OtaStatus::Failure(OtaError::Internal(_), _)
        ));
    }

    #[tokio::test]
    async fn try_to_deployed_fail_signal() {
        let state_mock = MockStateRepository::<PersistentState>::new();
        let mut system_update = MockSystemUpdate::new();

        system_update.expect_install_bundle().returning(|_| Ok(()));
        system_update
            .expect_operation()
            .returning(|| Ok("".to_string()));
        system_update
            .expect_receive_completed()
            .returning(|| deploy_status_stream([DeployStatus::Completed { signal: -1 }]));
        system_update
            .expect_last_error()
            .returning(|| Ok("Unable to deploy image".to_string()));

        let (ota_status_publisher, _) = mpsc::channel(1);
        let (ota, _dir) = Ota::mock_new_with_path(
            system_update,
            state_mock,
            "deployed_fail_signal",
            ota_status_publisher,
        );

        let ota_status = ota.deploy(OtaId::default()).await;

        assert!(matches!(
            ota_status,
            OtaStatus::Failure(OtaError::InvalidBaseImage(_), _)
        ));
    }

    #[tokio::test]
    async fn try_to_deployed_success() {
        let state_mock = MockStateRepository::<PersistentState>::new();
        let mut system_update = MockSystemUpdate::new();

        system_update.expect_install_bundle().returning(|_| Ok(()));
        system_update
            .expect_operation()
            .returning(|| Ok("".to_string()));
        system_update.expect_receive_completed().returning(|| {
            let progress = [
                DeployStatus::Progress(DeployProgress {
                    percentage: 50,
                    message: "Copy image".to_string(),
                }),
                DeployStatus::Progress(DeployProgress {
                    percentage: 100,
                    message: "Installing is done".to_string(),
                }),
                DeployStatus::Completed { signal: 0 },
            ]
            .map(Ok);

            Ok(futures::stream::iter(progress).boxed())
        });

        let ota_request = OtaId::default();

        let (ota_status_publisher, mut ota_status_receiver) = mpsc::channel(3);
        let (ota, _dir) = Ota::mock_new_with_path(
            system_update,
            state_mock,
            "deployed_success",
            ota_status_publisher,
        );

        assert!(matches!(ota.ota_status, OtaStatus::Deployed(_)));

        let receive_result = ota_status_receiver.try_recv();
        assert!(receive_result.is_ok());
        let ota_status_received = receive_result.unwrap();
        assert!(matches!(ota_status_received, OtaStatus::Deploying(_, _)));

        let receive_result = ota_status_receiver.try_recv();
        assert!(receive_result.is_ok());
        let ota_status_received = receive_result.unwrap();
        assert!(matches!(ota_status_received, OtaStatus::Deploying(_, _)));

        let receive_result = ota_status_receiver.try_recv();
        assert!(receive_result.is_ok());
        let ota_status_received = receive_result.unwrap();
        assert!(matches!(ota_status_received, OtaStatus::Deployed(_)));

        let receive_result = ota_status_receiver.try_recv();
        assert!(receive_result.is_err());
    }

    #[tokio::test]
    async fn try_to_rebooting_success() {
        let state_mock = MockStateRepository::<PersistentState>::new();
        let system_update = MockSystemUpdate::new();
        let ota_request = OtaId::default();

        let (ota_status_publisher, mut ota_status_receiver) = mpsc::channel(1);
        let ota = Ota::mock_new(system_update, state_mock, ota_status_publisher);
        let ota_status = ota.reboot(ota_request).await;

        let receive_result = ota_status_receiver.try_recv();
        assert!(receive_result.is_ok());
        let ota_status_received = receive_result.unwrap();
        assert!(matches!(ota_status_received, OtaStatus::Rebooting(_)));

        let receive_result = ota_status_receiver.try_recv();
        assert!(receive_result.is_err());

        assert!(matches!(ota_status, OtaStatus::Rebooted));
    }

    #[tokio::test]
    async fn try_to_success_no_pending_update() {
        let mut state_mock = MockStateRepository::<PersistentState>::new();
        let system_update = MockSystemUpdate::new();

        state_mock.expect_exists().returning(|| false);

        let (ota_status_publisher, _ota_status_receiver) = mpsc::channel(1);
        let ota = Ota::mock_new(system_update, state_mock, ota_status_publisher);
        let ota_status = ota.success().await;

        assert!(matches!(ota_status, OtaStatus::NoPendingOta));
    }

    #[tokio::test]
    async fn try_to_success_fail_read_state() {
        let mut state_mock = MockStateRepository::<PersistentState>::new();
        let system_update = MockSystemUpdate::new();

        state_mock.expect_exists().returning(|| true);
        state_mock.expect_read().returning(move || {
            Err(FileStateError::Write {
                path: "/ota.bin".into(),
                backtrace: io::Error::new(io::ErrorKind::PermissionDenied, "permission denied"),
            })
        });

        let (ota_status_publisher, _ota_status_receiver) = mpsc::channel(1);
        let ota = Ota::mock_new(system_update, state_mock, ota_status_publisher);
        let ota_status = ota.success().await;

        assert!(matches!(ota_status, OtaStatus::Failure(OtaError::Io(_), _)));
    }

    #[tokio::test]
    async fn try_to_success_fail_pending_ota() {
        let mut state_mock = MockStateRepository::<PersistentState>::new();
        let mut system_update = MockSystemUpdate::new();
        let uuid = Uuid::new_v4();
        let slot = "A";

        state_mock.expect_exists().returning(|| true);
        state_mock.expect_read().returning(move || {
            Ok(PersistentState {
                uuid,
                slot: slot.to_owned(),
            })
        });
        state_mock.expect_clear().returning(|| Ok(()));

        system_update
            .expect_boot_slot()
            .returning(|| Ok("A".to_owned()));

        let (ota_status_publisher, _ota_status_receiver) = mpsc::channel(1);
        let ota = Ota::mock_new(system_update, state_mock, ota_status_publisher);
        let ota_status = ota.success().await;

        assert!(matches!(
            ota_status,
            OtaStatus::Failure(OtaError::SystemRollback(_), _)
        ));
    }

    #[tokio::test]
    async fn try_to_success() {
        let mut state_mock = MockStateRepository::<PersistentState>::new();
        let mut system_update = MockSystemUpdate::new();
        let uuid = Uuid::new_v4();
        let slot = "A";

        state_mock.expect_exists().returning(|| true);
        state_mock.expect_read().returning(move || {
            Ok(PersistentState {
                uuid,
                slot: slot.to_owned(),
            })
        });
        state_mock.expect_clear().returning(|| Ok(()));

        system_update
            .expect_boot_slot()
            .returning(|| Ok("B".to_owned()));
        system_update
            .expect_get_primary()
            .returning(|| Ok("rootfs.0".to_owned()));
        system_update.expect_mark().returning(|_: &str, _: &str| {
            Ok((
                "rootfs.0".to_owned(),
                "marked slot rootfs.0 as good".to_owned(),
            ))
        });

        let (ota_status_publisher, _ota_status_receiver) = mpsc::channel(1);
        let ota = Ota::mock_new(system_update, state_mock, ota_status_publisher);
        let ota_status = ota.success().await;

        assert!(matches!(ota_status, OtaStatus::Success(_)));
    }

    #[tokio::test]
    async fn do_pending_ota_fail_boot_slot() {
        let mut state_mock = MockStateRepository::<PersistentState>::new();
        let uuid = Uuid::new_v4();
        let slot = "A";

        state_mock.expect_read().returning(move || {
            Ok(PersistentState {
                uuid,
                slot: slot.to_owned(),
            })
        });

        let mut system_update = MockSystemUpdate::new();
        system_update.expect_boot_slot().returning(|| {
            Err(DeviceManagerError::Fatal(
                "unable to call boot slot".to_string(),
            ))
        });

        let (ota_status_publisher, _ota_status_receiver) = mpsc::channel(1);
        let ota = Ota::mock_new(system_update, state_mock, ota_status_publisher);

        let state = ota.state_repository.read().await.unwrap();
        let result = ota.do_pending_ota(&state).await;

        assert!(result.is_err());
        assert!(matches!(result.err().unwrap(), OtaError::Internal(_),));
    }

    #[tokio::test]
    async fn do_pending_ota_fail_switch_slot() {
        let mut state_mock = MockStateRepository::<PersistentState>::new();
        let uuid = Uuid::new_v4();
        let slot = "A";

        state_mock.expect_read().returning(move || {
            Ok(PersistentState {
                uuid,
                slot: slot.to_owned(),
            })
        });

        let mut system_update = MockSystemUpdate::new();
        system_update
            .expect_boot_slot()
            .returning(|| Ok("A".to_owned()));

        let (ota_status_publisher, _ota_status_receiver) = mpsc::channel(1);
        let ota = Ota::mock_new(system_update, state_mock, ota_status_publisher);

        let state = ota.state_repository.read().await.unwrap();
        let result = ota.do_pending_ota(&state).await;

        assert!(result.is_err());
        assert!(matches!(result.err().unwrap(), OtaError::SystemRollback(_),));
    }

    #[tokio::test]
    async fn do_pending_ota_fail_get_primary() {
        let mut state_mock = MockStateRepository::<PersistentState>::new();
        let uuid = Uuid::new_v4();
        let slot = "A";

        state_mock.expect_read().returning(move || {
            Ok(PersistentState {
                uuid,
                slot: slot.to_owned(),
            })
        });

        let mut system_update = MockSystemUpdate::new();
        system_update
            .expect_boot_slot()
            .returning(|| Ok("B".to_owned()));
        system_update.expect_get_primary().returning(|| {
            Err(DeviceManagerError::Fatal(
                "unable to call boot slot".to_string(),
            ))
        });

        let (ota_status_publisher, _ota_status_receiver) = mpsc::channel(1);
        let ota = Ota::mock_new(system_update, state_mock, ota_status_publisher);
        let state = ota.state_repository.read().await.unwrap();
        let result = ota.do_pending_ota(&state).await;

        assert!(result.is_err());
        assert!(matches!(result.err().unwrap(), OtaError::Internal(_),));
    }

    #[tokio::test]
    async fn do_pending_ota_mark_slot_fail() {
        let uuid = Uuid::new_v4();
        let slot = "A";

        let mut state_mock = MockStateRepository::<PersistentState>::new();
        state_mock.expect_exists().returning(|| true);
        state_mock.expect_read().returning(move || {
            Ok(PersistentState {
                uuid,
                slot: slot.to_owned(),
            })
        });

        state_mock.expect_clear().returning(|| Ok(()));

        let mut system_update = MockSystemUpdate::new();
        system_update
            .expect_boot_slot()
            .returning(|| Ok("B".to_owned()));
        system_update
            .expect_get_primary()
            .returning(|| Ok("rootfs.0".to_owned()));
        system_update.expect_mark().returning(|_: &str, _: &str| {
            Err(DeviceManagerError::Fatal(
                "Unable to call mark function".to_string(),
            ))
        });

        let (ota_status_publisher, _ota_status_receiver) = mpsc::channel(1);
        let ota = Ota::mock_new(system_update, state_mock, ota_status_publisher);

        let state = ota.state_repository.read().await.unwrap();
        let result = ota.do_pending_ota(&state).await;
        assert!(result.is_err());
        assert!(matches!(result.err().unwrap(), OtaError::Internal(_),));
    }

    #[tokio::test]
    async fn do_pending_ota_fail_marked_wrong_slot() {
        let uuid = Uuid::new_v4();
        let slot = "A";

        let mut state_mock = MockStateRepository::<PersistentState>::new();
        state_mock.expect_exists().returning(|| true);
        state_mock.expect_read().returning(move || {
            Ok(PersistentState {
                uuid,
                slot: slot.to_owned(),
            })
        });

        state_mock.expect_clear().returning(|| Ok(()));

        let mut system_update = MockSystemUpdate::new();
        system_update
            .expect_boot_slot()
            .returning(|| Ok("B".to_owned()));
        system_update
            .expect_get_primary()
            .returning(|| Ok("rootfs.0".to_owned()));
        system_update.expect_mark().returning(|_: &str, _: &str| {
            Ok((
                "rootfs.1".to_owned(),
                "marked slot rootfs.1 as good".to_owned(),
            ))
        });

        let (ota_status_publisher, _ota_status_receiver) = mpsc::channel(1);
        let ota = Ota::mock_new(system_update, state_mock, ota_status_publisher);

        let state = ota.state_repository.read().await.unwrap();
        let result = ota.do_pending_ota(&state).await;
        assert!(result.is_err());
        assert!(matches!(result.err().unwrap(), OtaError::Internal(_),));
    }

    #[tokio::test]
    async fn do_pending_ota_success() {
        let uuid = Uuid::new_v4();
        let slot = "A";

        let mut state_mock = MockStateRepository::<PersistentState>::new();
        state_mock.expect_exists().returning(|| true);
        state_mock.expect_read().returning(move || {
            Ok(PersistentState {
                uuid,
                slot: slot.to_owned(),
            })
        });

        state_mock.expect_clear().returning(|| Ok(()));

        let mut system_update = MockSystemUpdate::new();
        system_update
            .expect_boot_slot()
            .returning(|| Ok("B".to_owned()));
        system_update
            .expect_get_primary()
            .returning(|| Ok("rootfs.0".to_owned()));
        system_update.expect_mark().returning(|_: &str, _: &str| {
            Ok((
                "rootfs.0".to_owned(),
                "marked slot rootfs.0 as good".to_owned(),
            ))
        });

        let (ota_status_publisher, _ota_status_receiver) = mpsc::channel(1);
        let ota = Ota::mock_new(system_update, state_mock, ota_status_publisher);

        let state = ota.state_repository.read().await.unwrap();
        let result = ota.do_pending_ota(&state).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn wget_failed() {
        let (_dir, t_dir) = temp_dir("wget_failed");

        let server = MockServer::start_async().await;
        let hello_mock = server
            .mock_async(|when, then| {
                when.method(GET);
                then.status(500);
            })
            .await;

        let ota_file = t_dir.join("ota,bin");
        let (ota_status_publisher, _) = mpsc::channel(1);

        let result = wget(
            server.url("/ota.bin").as_str(),
            &ota_file,
            &Uuid::new_v4(),
            &ota_status_publisher,
        )
        .await;

        hello_mock.assert_async().await;
        assert!(result.is_err());
        assert!(matches!(result.err().unwrap(), OtaError::Network(_),));
    }

    #[tokio::test]
    async fn wget_failed_wrong_content_length() {
        let (_dir, t_dir) = temp_dir("wget_failed_wrong_content_length");

        let binary_content = b"\x80\x02\x03";

        let server = MockServer::start_async().await;
        let ota_url = server.url("/ota.bin");
        let mock_ota_file_request = server
            .mock_async(|when, then| {
                when.method(GET).path("/ota.bin");
                then.status(200)
                    .header("content-Length", 0.to_string())
                    .body(binary_content);
            })
            .await;

        let ota_file = t_dir.join("ota.bin");
        let uuid_request = Uuid::new_v4();

        let (ota_status_publisher, _) = mpsc::channel(1);

        let result = wget(
            ota_url.as_str(),
            &ota_file,
            &uuid_request,
            &ota_status_publisher,
        )
        .await;

        mock_ota_file_request.assert_async().await;
        assert!(result.is_err());
        assert!(matches!(result.err().unwrap(), OtaError::Network(_),));
    }

    #[tokio::test]
    async fn wget_with_empty_payload() {
        let (_dir, t_dir) = temp_dir("wget_with_empty_payload");

        let server = MockServer::start_async().await;
        let mock_ota_file_request = server
            .mock_async(|when, then| {
                when.method(GET).path("/ota.bin");
                then.status(200).body(b"");
            })
            .await;

        let ota_file = t_dir.join("ota.bin");
        let (ota_status_publisher, _) = mpsc::channel(1);

        let result = wget(
            server.url("/ota.bin").as_str(),
            &ota_file,
            &Uuid::new_v4(),
            &ota_status_publisher,
        )
        .await;

        mock_ota_file_request.assert_async().await;
        assert!(result.is_err());
        assert!(matches!(result.err().unwrap(), OtaError::Network(_),));
    }

    #[tokio::test]
    async fn wget_success() {
        let (_dir, t_dir) = temp_dir("wget_success");

        let binary_content = b"\x80\x02\x03";
        let binary_size = binary_content.len();

        let server = MockServer::start_async().await;
        let ota_url = server.url("/ota.bin");
        let mock_ota_file_request = server
            .mock_async(|when, then| {
                when.method(GET).path("/ota.bin");
                then.status(200)
                    .header("content-Length", binary_size.to_string())
                    .body(binary_content);
            })
            .await;

        let ota_file = t_dir.join("ota.bin");
        let uuid_request = Uuid::new_v4();

        let (ota_status_publisher, mut ota_status_receiver) = mpsc::channel(1);

        let result = wget(
            ota_url.as_str(),
            &ota_file,
            &uuid_request,
            &ota_status_publisher,
        )
        .await;
        mock_ota_file_request.assert_async().await;

        let receive_result = ota_status_receiver.try_recv();
        assert!(receive_result.is_ok());
        let ota_status_received = receive_result.unwrap();
        assert!(matches!(
            ota_status_received,
            OtaStatus::Downloading(_, 100)
        ));

        let receive_result = ota_status_receiver.try_recv();
        assert!(receive_result.is_err());

        assert!(result.is_ok());
    }
}
