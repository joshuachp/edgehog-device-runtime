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

//! Housekeeping task for the FileTransfer

use std::borrow::Cow;
use std::path::Path;

use edgehog_store::models::job::Job;
use edgehog_store::models::job::job_type::JobType;
use edgehog_store::models::job::status::JobStatus;
use eyre::{OptionExt, bail};
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, instrument};
use uuid::Uuid;
use zbus::address::transport::Unix;

use crate::jobs::Queue;

/// Task that handles scheduled task for the file transfer
pub struct StorageTask {
    queue: Queue,
    notify: Notify,
}

impl StorageTask {
    #[instrument(skip_all)]
    async fn run(self, cancel: CancellationToken) -> eyre::Result<()> {
        loop {
            self.jobs().await?;

            if !self.wait_next(&cancel).await {
                break;
            }
        }

        Ok(())
    }

    #[instrument(skip_all)]
    async fn wait_next(&self, cancel: &CancellationToken) -> bool {
        debug!("waiting for next job");

        cancel
            .run_until_cancelled(self.notify.notified())
            .await
            .is_some()
    }

    #[instrument(skip_all)]
    async fn jobs(&self) -> eyre::Result<()> {
        while let Some(job) = self.queue.next_scheduled_job(JobType::FileStorage).await? {
            if let Err(error) = self.handle(&job).await {
                error!(%error,"couldn't handle storage job");
            }
        }

        Ok(())
    }

    #[instrument(skip_all, fields(id = %job.id))]
    async fn handle(&self, job: &Job) -> eyre::Result<()> {
        todo!();

        Ok(())
    }
}

/// File cleanup job.
#[derive(Debug, minicbor::Encode, minicbor::Decode)]
struct CleanUp<'a> {
    #[cbor(skip)]
    pub(crate) id: Uuid,
    #[cbor(skip)]
    pub(crate) schedule_at: i64,
    #[n(0)]
    file_path: Cow<'a, Path>,
}

impl<'a> CleanUp<'a> {
    const SERIALIZED_VERSION: i32 = 0;
}

impl<'a> TryFrom<&'a Job> for CleanUp<'a> {
    type Error = eyre::Report;

    fn try_from(value: &'a Job) -> Result<Self, Self::Error> {
        let Job {
            id,
            job_type,
            status,
            version,
            tag,
            schedule_at,
            data,
        } = value;

        debug_assert_eq!(*job_type, JobType::FileStorage);
        debug_assert_eq!(*status, JobStatus::InProgress);
        debug_assert_eq!(*tag, i32::from(StorageJobTag::CleanUp));
        debug_assert_eq!(*version, Self::SERIALIZED_VERSION);

        let mut this: Self = minicbor::decode(&data)?;

        this.id = **id;
        this.schedule_at = schedule_at.ok_or_eyre("missing schedule_at field")?;

        Ok(this)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
#[repr(u8)]
pub enum StorageJobTag {
    CleanUp = 0,
}

impl From<StorageJobTag> for i32 {
    fn from(value: StorageJobTag) -> Self {
        value as i32
    }
}

impl TryFrom<i32> for StorageJobTag {
    type Error = eyre::Report;

    fn try_from(value: i32) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(StorageJobTag::CleanUp),
            _ => bail!("unrecognize file transfer job tag {value}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::tests::with_insta;
    use rstest::{Context, rstest};

    use super::*;

    #[rstest]
    #[case(StorageJobTag::CleanUp)]
    fn job_tag_roundtrip(#[context] ctx: Context, #[case] value: StorageJobTag) {
        let buf = i32::from(value);

        let res = StorageJobTag::try_from(buf).unwrap();

        assert_eq!(res, value);

        with_insta!({
            let name = format!("{}_{}", ctx.name, ctx.case.unwrap());

            insta::assert_snapshot!(name, format!("{value:?} = {}", buf));
        });
    }
}
