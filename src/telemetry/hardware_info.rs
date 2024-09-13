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

use crate::data::{publish, Publisher};
use astarte_device_sdk::types::AstarteType;
use log::error;
use procfs::{CpuInfo, Meminfo, ProcError, ProcResult};

#[derive(Debug)]
pub struct HardwareInfo<'a, T> {
    client: &'a T,
}

impl<'a, T> HardwareInfo<'a, T> {
    const INTERFACE: &'static str = "io.edgehog.devicemanager.HardwareInfo";

    pub fn new(client: &'a T) -> Self {
        Self { client }
    }

    /// get structured data for `io.edgehog.devicemanager.HardwareInfo` interface
    pub async fn send(&self)
    where
        T: Publisher,
    {
        let architecture = get_machine_architecture();
        self.publish_prop("/cpu/architecture", architecture).await;

        if let Err(err) = self.publish_cpu_info().await {
            error!("couldn't get cpu info: {}", stable_eyre::Report::new(err));
        }

        if let Err(err) = self.publish_mem_info().await {
            error!("couldn't get mem info: {}", stable_eyre::Report::new(err));
        }
    }

    async fn publish_mem_info(&self) -> Result<(), ProcError>
    where
        T: Publisher,
    {
        let mem_info = get_meminfo()?;

        if let Ok(mem_total) = i64::try_from(mem_info.mem_total) {
            self.publish_prop("/mem/totalBytes", mem_total).await;
        } else {
            error!(
                "mem total too big to be sent to astarte: {}",
                mem_info.mem_total
            )
        }

        Ok(())
    }

    async fn publish_cpu_info(&self) -> Result<(), ProcError>
    where
        T: Publisher,
    {
        let mut cpu_info = get_cpu_info()?;

        if let Some(model) = cpu_info.fields.remove("model") {
            self.publish_prop("/cpu/model", model).await;
        }

        if let Some(model_name) = cpu_info.fields.remove("model name") {
            self.publish_prop("/cpu/modelName", model_name).await;
        }

        if let Some(vendor_id) = cpu_info.fields.remove("vendor_id") {
            self.publish_prop("/cpu/vendor", vendor_id).await;
        }

        Ok(())
    }

    async fn publish_prop(&self, path: &str, data: impl Into<AstarteType>)
    where
        T: Publisher,
    {
        publish(self.client, Self::INTERFACE, path, data).await;
    }
}

#[cfg(not(test))]
fn get_cpu_info() -> ProcResult<CpuInfo> {
    use procfs::Current;

    procfs::CpuInfo::current()
}

#[cfg(not(test))]
fn get_machine_architecture() -> String {
    std::env::consts::ARCH.to_owned()
}

#[cfg(not(test))]
fn get_meminfo() -> ProcResult<Meminfo> {
    use procfs::Current;

    procfs::Meminfo::current()
}

#[cfg(test)]
fn get_cpu_info() -> ProcResult<CpuInfo> {
    use procfs::FromRead;

    let data = r#"processor       : 0
vendor_id       : GenuineIntel
model           : 158
model name      : ARMv7 Processor rev 10 (v7l)
BogoMIPS        : 6.00
Features        : half thumb fastmult vfp edsp neon vfpv3 tls vfpd32
CPU implementer : 0x41
CPU architecture: 7
CPU variant     : 0x2
CPU part        : 0xc09
CPU revision    : 10

Hardware        : Freescale i.MX6 SoloX (Device Tree)
Revision        : 0000
Serial          : 0000000000000000
"#;

    let r = std::io::Cursor::new(data.as_bytes());

    Ok(CpuInfo::from_read(r).unwrap())
}

#[cfg(test)]
fn get_machine_architecture() -> String {
    "test_architecture".to_owned()
}

#[cfg(test)]
fn get_meminfo() -> ProcResult<Meminfo> {
    use procfs::FromRead;

    let data = r#"MemTotal:        1019356 kB
MemFree:          739592 kB
MemAvailable:     802296 kB
Buffers:            7372 kB
Cached:            88364 kB
SwapCached:            0 kB
Active:            41328 kB
Inactive:          64908 kB
Active(anon):       1224 kB
Inactive(anon):    35160 kB
Active(file):      40104 kB
Inactive(file):    29748 kB
Unevictable:           0 kB
Mlocked:               0 kB
HighTotal:             0 kB
HighFree:              0 kB
LowTotal:        1019356 kB
LowFree:          739592 kB
SwapTotal:             0 kB
SwapFree:              0 kB
Dirty:                 4 kB
Writeback:             0 kB
AnonPages:         10500 kB
Mapped:            21688 kB
Shmem:             25884 kB
KReclaimable:       8452 kB
Slab:              21180 kB
SReclaimable:       8452 kB
SUnreclaim:        12728 kB
KernelStack:         752 kB
PageTables:          656 kB
NFS_Unstable:          0 kB
Bounce:                0 kB
WritebackTmp:          0 kB
CommitLimit:      509676 kB
Committed_AS:     139696 kB
VmallocTotal:    1032192 kB
VmallocUsed:        6648 kB
VmallocChunk:          0 kB
Percpu:              376 kB
CmaTotal:         327680 kB
CmaFree:          194196 kB
"#;

    let r = std::io::Cursor::new(data.as_bytes());
    Ok(Meminfo::from_read(r).unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::data::tests::MockPubSub;

    #[tokio::test]
    async fn hardware_info_test() {
        let mut mock = MockPubSub::new();

        mock.expect_send()
            .times(1)
            .withf(|interface, path, data| {
                interface == "io.edgehog.devicemanager.HardwareInfo"
                    && path == "/cpu/architecture"
                    && *data == AstarteType::String("test_architecture".to_string())
            })
            .returning(|_, _, _| Ok(()));

        mock.expect_send()
            .times(1)
            .withf(|interface, path, data| {
                interface == "io.edgehog.devicemanager.HardwareInfo"
                    && path == "/cpu/model"
                    && *data == AstarteType::String("158".to_string())
            })
            .returning(|_, _, _| Ok(()));

        mock.expect_send()
            .times(1)
            .withf(|interface, path, data| {
                interface == "io.edgehog.devicemanager.HardwareInfo"
                    && path == "/cpu/modelName"
                    && *data == AstarteType::String("ARMv7 Processor rev 10 (v7l)".to_string())
            })
            .returning(|_, _, _| Ok(()));

        mock.expect_send()
            .times(1)
            .withf(|interface, path, data| {
                interface == "io.edgehog.devicemanager.HardwareInfo"
                    && path == "/cpu/vendor"
                    && *data == AstarteType::String("GenuineIntel".to_string())
            })
            .returning(|_, _, _| Ok(()));

        mock.expect_send()
            .times(1)
            .withf(|interface, path, data| {
                interface == "io.edgehog.devicemanager.HardwareInfo"
                    && path == "/mem/totalBytes"
                    && *data == AstarteType::LongInteger(1043820544)
            })
            .returning(|_, _, _| Ok(()));

        HardwareInfo::new(&mock).send().await;
    }
}
