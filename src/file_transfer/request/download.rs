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

//! File Transfer download request

use std::time::Duration;

use edgehog_store::conversions::SqlUuid;
use edgehog_store::models::job::Job;
use edgehog_store::models::job::job_type::JobType;
use edgehog_store::models::job::status::JobStatus;
use eyre::{Context, OptionExt};
use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderName, HeaderValue};
use url::Url;
use uuid::Uuid;

use super::{Compression, FileDigest, FileOptions, FilePermissions, Target, conv_or_default};
use crate::file_transfer::interface::ServerToDevice;
use crate::file_transfer::request::JobTag;

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DownloadReq {
    pub(crate) id: Uuid,
    pub(crate) url: Url,
    pub(crate) headers: HeaderMap,
    pub(crate) progress: bool,
    pub(crate) digest_type: FileDigest,
    pub(crate) digest: Vec<u8>,
    pub(crate) ttl: Option<Duration>,
    pub(crate) compression: Option<Compression>,
    pub(crate) file_size: u64,
    pub(crate) permission: FilePermissions,
    pub(crate) destination: Target,
}

impl DownloadReq {
    const SERIALIZED_VERSION: i32 = 0;
}

impl From<DownloadReq> for FileOptions {
    fn from(value: DownloadReq) -> Self {
        FileOptions {
            id: value.id,
            file_size: value.file_size,
            file_digest: value.digest_type,
            #[cfg(unix)]
            perm: value.permission,
        }
    }
}

impl TryFrom<ServerToDevice> for DownloadReq {
    type Error = eyre::Error;

    fn try_from(value: ServerToDevice) -> Result<Self, Self::Error> {
        let ServerToDevice {
            id,
            url,
            http_header_key,
            http_header_value,
            compression,
            file_size_bytes,
            progress,
            digest,
            ttl_seconds,
            file_mode,
            user_id,
            group_id,
            destination,
        } = value;

        let headers = http_header_key
            .into_iter()
            .zip(http_header_value)
            .map(|(k, v)| -> eyre::Result<(HeaderName, HeaderValue)> {
                let k = HeaderName::try_from(k)?;
                let mut v = HeaderValue::try_from(v)?;

                if k == AUTHORIZATION {
                    v.set_sensitive(true);
                }

                Ok((k, v))
            })
            .collect::<eyre::Result<HeaderMap>>()?;

        let ttl = conv_or_default(ttl_seconds, 0)
            .wrap_err("couldn't convert ttl_seconds to duration")?
            .map(Duration::from_secs);

        let permission = FilePermissions::from_event(file_mode, user_id, group_id)?;

        let file_size = u64::try_from(file_size_bytes).wrap_err("couldn't convert file size")?;

        let (digest_type, digest) = digest
            .split_once(':')
            .ok_or_eyre("couldn't parse digest, missing ':' delimiter")?;

        let digest = hex::decode(digest).wrap_err("couldn't decode hex digest")?;

        let compression = (!compression.is_empty())
            .then(|| compression.parse())
            .transpose()?;

        Ok(Self {
            id: id.parse()?,
            url: url.parse()?,
            headers,
            compression,
            file_size,
            progress,
            digest_type: digest_type.parse()?,
            digest,
            ttl,
            destination: destination.parse()?,
            permission,
        })
    }
}

impl TryFrom<&DownloadReq> for Job {
    type Error = eyre::Report;

    fn try_from(value: &DownloadReq) -> Result<Self, Self::Error> {
        let DownloadReq {
            id,
            url,
            headers,
            progress,
            digest_type,
            digest,
            ttl,
            compression,
            file_size,
            permission:
                FilePermissions {
                    mode,
                    user_id,
                    group_id,
                },
            destination,
        } = value;

        let buf = Vec::new();
        let mut encoder = minicbor::Encoder::new(buf);

        encoder.str(url.as_str())?;
        encoder.map(headers.len().try_into()?)?;

        for (k, v) in headers.iter() {
            encoder.str(k.as_str())?.bytes(v.as_bytes())?;
        }

        encoder
            .bool(*progress)?
            .u8(u8::from(*digest_type))?
            .bytes(&digest)?;

        match ttl {
            Some(ttl) => {
                encoder.u64(ttl.as_secs())?;
            }
            None => {
                encoder.null()?;
            }
        }

        match compression {
            Some(compression) => {
                encoder.u8(u8::from(*compression))?;
            }
            None => {
                encoder.null()?;
            }
        }

        encoder.u64(*file_size)?;

        match mode {
            Some(mode) => {
                encoder.u32(*mode)?;
            }
            None => {
                encoder.null()?;
            }
        }

        match user_id {
            Some(user_id) => {
                encoder.u32(*user_id)?;
            }
            None => {
                encoder.null()?;
            }
        }

        match group_id {
            Some(group_id) => {
                encoder.u32(*group_id)?;
            }
            None => {
                encoder.null()?;
            }
        }

        encoder.u8(u8::from(*destination))?;

        Ok(Job {
            id: SqlUuid::new(*id),
            job_type: JobType::FileTransfer,
            status: JobStatus::default(),
            version: DownloadReq::SERIALIZED_VERSION,
            tag: JobTag::Download.into(),
            data: encoder.into_writer(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use rstest::{fixture, rstest};

    use crate::file_transfer::interface::tests::fs_server_to_device;

    #[fixture]
    pub(crate) fn download_req() -> DownloadReq {
        DownloadReq {
            id: "6389218e-0e05-4587-96e3-3e6e2b522a2b".parse().unwrap(),
            url: "https://s3.example.com".parse().unwrap(),
            headers: HeaderMap::from_iter([(
                AUTHORIZATION,
                HeaderValue::from_static(
                    "Bearer tXYBVo1eA+8MTQTgFovzb9/nKej1d7zS4/k64l3Tm7tOkzxGemBJqDKN5lhEr1ARkb6AXpMqRc6FKo3kk800kA==",
                ),
            )]),
            file_size: 4096,
            compression: Some(Compression::TarGz),
            progress: true,
            digest_type: FileDigest::Sha256,
            digest: hex::decode("28babb1cdf8aea6b62acc1097fdc83482cbf6e11c4fe7dcb39ae1682776baec5")
                .unwrap(),
            ttl: None,
            permission: FilePermissions {
                mode: Some(544),
                user_id: Some(1000),
                group_id: Some(100),
            },
            destination: Target::Storage,
        }
    }

    #[rstest]
    fn download_try_from_event(fs_server_to_device: ServerToDevice, download_req: DownloadReq) {
        let req = DownloadReq::try_from(fs_server_to_device).unwrap();

        assert_eq!(req, download_req);
    }
}
