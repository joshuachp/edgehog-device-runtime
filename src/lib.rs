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

use std::path::PathBuf;

use data::astarte_device_sdk_lib::AstarteDeviceSdkConfigOptions;
use serde::Deserialize;
use telemetry::TelemetryInterfaceConfig;

use crate::error::DeviceManagerError;

mod commands;
pub mod controller;
pub mod data;
mod device;
pub mod error;
#[cfg(feature = "forwarder")]
mod forwarder;
mod led_behavior;
mod ota;
mod power_management;
pub mod repository;
#[cfg(feature = "systemd")]
pub mod systemd_wrapper;
pub mod telemetry;

const MAX_OTA_OPERATION: usize = 2;

#[derive(Deserialize, Debug, Clone)]
pub enum AstarteLibrary {
    #[serde(rename = "astarte-device-sdk")]
    AstarteDeviceSdk,
    #[cfg(feature = "message-hub")]
    #[serde(rename = "astarte-message-hub")]
    AstarteMessageHub,
}

#[derive(Debug, Deserialize, Clone)]
pub struct DeviceManagerOptions {
    pub astarte_library: AstarteLibrary,
    pub astarte_device_sdk: Option<AstarteDeviceSdkConfigOptions>,
    #[cfg(feature = "message-hub")]
    pub astarte_message_hub: Option<data::astarte_message_hub_node::AstarteMessageHubOptions>,
    pub interfaces_directory: PathBuf,
    pub store_directory: PathBuf,
    pub download_directory: PathBuf,
    pub telemetry_config: Option<Vec<TelemetryInterfaceConfig<'static>>>,
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use astarte_device_sdk::types::AstarteType;
    use url::Url;

    use crate::data::astarte_device_sdk_lib::AstarteDeviceSdkConfigOptions;
    use crate::data::tests::create_tmp_store;
    use crate::data::tests::MockPubSub;
    use crate::telemetry::battery_status::{get_battery_status, BatteryStatus};
    use crate::telemetry::net_interfaces::send_network_interface_properties;
    use crate::telemetry::storage_usage::{get_storage_usage, DiskUsage};
    use crate::telemetry::system_status::{get_system_status, SystemStatus};
    use crate::{AstarteLibrary, DeviceManagerOptions};

    #[cfg(feature = "forwarder")]
    fn mock_forwarder(
        publisher: &mut MockPubSub,
    ) -> &mut crate::data::tests::__mock_MockPubSub_Clone::__clone::Expectation {
        // define an expectation for the cloned MockPublisher due to the `init` method of the
        // Forwarder struct
        publisher.expect_clone().returning(move || {
            let mut publisher_clone = MockPubSub::new();

            publisher_clone
                .expect_interface_props()
                .withf(move |iface: &str| iface == "io.edgehog.devicemanager.ForwarderSessionState")
                .returning(|_: &str| Ok(Vec::new()));

            publisher_clone
        })
    }

    #[tokio::test]
    #[should_panic]
    async fn device_new_sdk_panic_fail() {
        let (store, store_dir) = create_tmp_store().await;

        let options = DeviceManagerOptions {
            astarte_library: AstarteLibrary::AstarteDeviceSdk,
            astarte_device_sdk: Some(AstarteDeviceSdkConfigOptions {
                realm: "".to_string(),
                device_id: Some("device_id".to_string()),
                credentials_secret: Some("credentials_secret".to_string()),
                pairing_url: Url::parse("http://[::]").unwrap(),
                pairing_token: None,
                ignore_ssl: false,
            }),
            #[cfg(feature = "message-hub")]
            astarte_message_hub: None,
            interfaces_directory: PathBuf::new(),
            store_directory: store_dir.path().to_owned(),
            download_directory: PathBuf::new(),
            telemetry_config: Some(vec![]),
        };

        let (pub_sub, handle) = options
            .astarte_device_sdk
            .as_ref()
            .unwrap()
            .connect(
                store,
                &options.store_directory,
                &options.interfaces_directory,
            )
            .await
            .unwrap();
        let dm = DeviceManager::start(options, pub_sub, handle).await;

        assert!(dm.is_ok());
    }

    #[tokio::test]
    async fn device_manager_new_success() {
        let options = DeviceManagerOptions {
            astarte_library: AstarteLibrary::AstarteDeviceSdk,
            astarte_device_sdk: Some(AstarteDeviceSdkConfigOptions {
                realm: "".to_string(),
                device_id: Some("device_id".to_string()),
                credentials_secret: Some("credentials_secret".to_string()),
                pairing_url: Url::parse("http://[::]").unwrap(),
                pairing_token: None,
                ignore_ssl: false,
            }),
            #[cfg(feature = "message-hub")]
            astarte_message_hub: None,
            interfaces_directory: PathBuf::new(),
            store_directory: PathBuf::new(),
            download_directory: PathBuf::new(),
            telemetry_config: Some(vec![]),
        };

        let mut pub_sub = MockPubSub::new();

        #[cfg(feature = "forwarder")]
        mock_forwarder(&mut pub_sub);

        pub_sub.expect_clone().returning(MockPubSub::new);

        let dm = DeviceManager::start(options, pub_sub, tokio::spawn(async { Ok(()) })).await;
        assert!(dm.is_ok(), "error {}", dm.err().unwrap());
    }

    #[tokio::test]
    async fn send_initial_telemetry_success() {
        let options = DeviceManagerOptions {
            astarte_library: AstarteLibrary::AstarteDeviceSdk,
            astarte_device_sdk: Some(AstarteDeviceSdkConfigOptions {
                realm: "".to_string(),
                device_id: Some("device_id".to_string()),
                credentials_secret: Some("credentials_secret".to_string()),
                pairing_url: Url::parse("http://[::]").unwrap(),
                pairing_token: None,
                ignore_ssl: false,
            }),
            #[cfg(feature = "message-hub")]
            astarte_message_hub: None,
            interfaces_directory: PathBuf::new(),
            store_directory: PathBuf::new(),
            download_directory: PathBuf::new(),
            telemetry_config: Some(vec![]),
        };

        let os_info = get_os_info().await.expect("failed to get os info");
        let mut pub_sub = MockPubSub::new();

        #[cfg(feature = "forwarder")]
        mock_forwarder(&mut pub_sub);

        pub_sub.expect_clone().returning(MockPubSub::new);

        pub_sub
            .expect_send()
            .withf(
                move |interface_name: &str, interface_path: &str, data: &AstarteType| {
                    interface_name == "io.edgehog.devicemanager.OSInfo"
                        && os_info.get(interface_path).unwrap() == data
                },
            )
            .returning(|_: &str, _: &str, _: AstarteType| Ok(()));

        let hardware_info = get_hardware_info().unwrap();
        pub_sub
            .expect_send()
            .withf(
                move |interface_name: &str, interface_path: &str, data: &AstarteType| {
                    interface_name == "io.edgehog.devicemanager.HardwareInfo"
                        && hardware_info.get(interface_path).unwrap() == data
                },
            )
            .returning(|_: &str, _: &str, _: AstarteType| Ok(()));

        let runtime_info = get_runtime_info().unwrap();
        pub_sub
            .expect_send()
            .withf(
                move |interface_name: &str, interface_path: &str, data: &AstarteType| {
                    interface_name == "io.edgehog.devicemanager.RuntimeInfo"
                        && runtime_info.get(interface_path).unwrap() == data
                },
            )
            .returning(|_: &str, _: &str, _: AstarteType| Ok(()));

        let storage_usage = get_storage_usage();
        pub_sub
            .expect_send_object()
            .withf(
                move |interface_name: &str, interface_path: &str, _: &DiskUsage| {
                    interface_name == "io.edgehog.devicemanager.StorageUsage"
                        && storage_usage.contains_key(&interface_path[1..])
                },
            )
            .returning(|_: &str, _: &str, _: DiskUsage| Ok(()));

        let network_iface_props = send_network_interface_properties().await.unwrap();
        pub_sub
            .expect_send()
            .withf(
                move |interface_name: &str, interface_path: &str, data: &AstarteType| {
                    interface_name == "io.edgehog.devicemanager.NetworkInterfaceProperties"
                        && network_iface_props.get(interface_path).unwrap() == data
                },
            )
            .returning(|_: &str, _: &str, _: AstarteType| Ok(()));

        let system_info = get_system_info().unwrap();
        pub_sub
            .expect_send()
            .withf(
                move |interface_name: &str, interface_path: &str, data: &AstarteType| {
                    interface_name == "io.edgehog.devicemanager.SystemInfo"
                        && system_info.get(interface_path).unwrap() == data
                },
            )
            .returning(|_: &str, _: &str, _: AstarteType| Ok(()));

        let base_image = get_base_image().await.expect("failed to get base image");
        pub_sub
            .expect_send()
            .withf(
                move |interface_name: &str, interface_path: &str, data: &AstarteType| {
                    interface_name == "io.edgehog.devicemanager.BaseImage"
                        && base_image.get(interface_path).unwrap() == data
                },
            )
            .returning(|_: &str, _: &str, _: AstarteType| Ok(()));

        let dm = DeviceManager::start(options, pub_sub, tokio::spawn(async { Ok(()) })).await;
        assert!(dm.is_ok());

        let telemetry_result = dm.unwrap().send_initial_telemetry().await;
        assert!(telemetry_result.is_ok());
    }

    #[tokio::test]
    async fn send_telemetry_success() {
        let system_status = get_system_status().unwrap();
        let mut pub_sub = MockPubSub::new();
        pub_sub
            .expect_send_object()
            .withf(
                move |interface_name: &str, interface_path: &str, _: &SystemStatus| {
                    interface_name == "io.edgehog.devicemanager.SystemStatus"
                        && interface_path == "/systemStatus"
                },
            )
            .returning(|_: &str, _: &str, _: SystemStatus| Ok(()));

        let storage_usage = get_storage_usage();
        pub_sub
            .expect_send_object()
            .withf(
                move |interface_name: &str, interface_path: &str, _: &DiskUsage| {
                    interface_name == "io.edgehog.devicemanager.StorageUsage"
                        && storage_usage.contains_key(&interface_path[1..])
                },
            )
            .returning(|_: &str, _: &str, _: DiskUsage| Ok(()));

        let battery_status = get_battery_status().await.unwrap();
        pub_sub
            .expect_send_object()
            .withf(
                move |interface_name: &str, interface_path: &str, _: &BatteryStatus| {
                    interface_name == "io.edgehog.devicemanager.BatteryStatus"
                        && battery_status.contains_key(&interface_path[1..])
                },
            )
            .returning(|_: &str, _: &str, _: BatteryStatus| Ok(()));

        DeviceManager::send_telemetry(
            &pub_sub,
            TelemetryMessage {
                path: "".to_string(),
                payload: TelemetryPayload::SystemStatus(system_status),
            },
        )
        .await;
        for (path, payload) in get_storage_usage() {
            DeviceManager::send_telemetry(
                &pub_sub,
                TelemetryMessage {
                    path,
                    payload: TelemetryPayload::StorageUsage(payload),
                },
            )
            .await;
        }
        for (path, payload) in get_battery_status().await.unwrap() {
            DeviceManager::send_telemetry(
                &pub_sub,
                TelemetryMessage {
                    path,
                    payload: TelemetryPayload::BatteryStatus(payload),
                },
            )
            .await;
        }
    }
}
