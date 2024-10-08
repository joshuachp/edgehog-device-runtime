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

use std::fmt::Display;

use tracing::{debug, error};
use udev::Device;

use crate::{
    data::{publish, Publisher},
    error::DeviceManagerError,
};

const INTERFACE: &str = "io.edgehog.devicemanager.NetworkInterfaceProperties";

const ARPHRD_ETHER: &str = "1";
const ARPHRD_PPP: &str = "512";

#[derive(Debug)]
enum TechnologyType {
    Ethernet,
    Cellular,
    WiFi,
}

impl Display for TechnologyType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TechnologyType::Ethernet => write!(f, "Ethernet"),
            TechnologyType::Cellular => write!(f, "Cellular"),
            TechnologyType::WiFi => write!(f, "WiFi"),
        }
    }
}

#[derive(Debug)]
struct NetworkInterface {
    interface: String,
    mac_address: String,
    technology_type: TechnologyType,
}

impl NetworkInterface {
    fn read_device(device: Device) -> Option<NetworkInterface> {
        device.property_value("ID_BUS")?;

        let addr = device.attribute_value("address")?;
        let technology_type = match device.attribute_value("type")?.to_str()? {
            ARPHRD_ETHER => {
                let uevent = device
                    .attribute_value("uevent")
                    .unwrap_or_default()
                    .to_string_lossy();

                if uevent.contains("DEVTYPE=wlan") {
                    TechnologyType::WiFi
                } else if uevent.contains("DEVTYPE=bridge") {
                    return None;
                } else {
                    TechnologyType::Ethernet
                }
            }
            ARPHRD_PPP => TechnologyType::Cellular,
            d_type => {
                debug!("unrecognized device type {d_type}");

                return None;
            }
        };

        Some(NetworkInterface {
            interface: device.sysname().to_string_lossy().to_string(),
            mac_address: addr.to_string_lossy().to_lowercase(),
            technology_type,
        })
    }

    async fn send<T>(self, client: &T)
    where
        T: Publisher,
    {
        publish(
            client,
            INTERFACE,
            &format!("/{}/macAddress", self.interface),
            self.mac_address,
        )
        .await;

        publish(
            client,
            INTERFACE,
            &format!("/{}/technologyType", self.interface),
            self.technology_type.to_string(),
        )
        .await;
    }
}

fn net_devices() -> Result<Vec<NetworkInterface>, DeviceManagerError> {
    let mut enumerator = udev::Enumerator::new()?;

    enumerator.match_subsystem("net")?;

    let list = enumerator.scan_devices()?;

    Ok(list.filter_map(NetworkInterface::read_device).collect())
}

/// get structured data for `io.edgehog.devicemanager.NetworkInterfaceProperties` interface
pub async fn send_network_interface_properties<T>(client: &T)
where
    T: Publisher,
{
    let devices = match net_devices() {
        Ok(devices) => devices,
        Err(err) => {
            error!(
                "couldn't get network interfaces: {}",
                stable_eyre::Report::new(err)
            );

            return;
        }
    };

    for nt_if in devices {
        nt_if.send(client).await;
    }
}

#[cfg(test)]
mod tests {
    use crate::data::tests::MockPubSub;

    use super::*;

    use astarte_device_sdk::types::AstarteType;
    use mockall::Sequence;

    #[test]
    fn technology_type_to_string_test() {
        assert_eq!(TechnologyType::Ethernet.to_string(), "Ethernet");
        assert_eq!(TechnologyType::Cellular.to_string(), "Cellular");
        assert_eq!(TechnologyType::WiFi.to_string(), "WiFi");
    }

    #[tokio::test]
    async fn network_interface_to_astarte_test() {
        let eth_wifi = vec![
            NetworkInterface {
                interface: "wifi_test".to_string(),
                mac_address: "00:11:22:33:44:55".to_string(),
                technology_type: TechnologyType::WiFi,
            },
            NetworkInterface {
                interface: "eth_test".to_string(),
                mac_address: "11:22:33:44:55:66".to_string(),
                technology_type: TechnologyType::Ethernet,
            },
            NetworkInterface {
                interface: "cellular_test".to_string(),
                mac_address: "22:33:44:55:66:77".to_string(),
                technology_type: TechnologyType::Cellular,
            },
        ];

        let mut client = MockPubSub::new();

        let mut seq = Sequence::new();

        client
            .expect_send()
            .times(1)
            .in_sequence(&mut seq)
            .withf(|interface, path, data| {
                interface == "io.edgehog.devicemanager.NetworkInterfaceProperties"
                    && path == "/wifi_test/macAddress"
                    && *data == AstarteType::String("00:11:22:33:44:55".to_string())
            })
            .returning(|_, _, _| Ok(()));

        client
            .expect_send()
            .times(1)
            .in_sequence(&mut seq)
            .withf(|interface, path, data| {
                interface == "io.edgehog.devicemanager.NetworkInterfaceProperties"
                    && path == "/wifi_test/technologyType"
                    && *data == AstarteType::String("WiFi".to_string())
            })
            .returning(|_, _, _| Ok(()));

        client
            .expect_send()
            .times(1)
            .in_sequence(&mut seq)
            .withf(|interface, path, data| {
                interface == "io.edgehog.devicemanager.NetworkInterfaceProperties"
                    && path == "/eth_test/macAddress"
                    && *data == AstarteType::String("11:22:33:44:55:66".to_string())
            })
            .returning(|_, _, _| Ok(()));

        client
            .expect_send()
            .times(1)
            .in_sequence(&mut seq)
            .withf(|interface, path, data| {
                interface == "io.edgehog.devicemanager.NetworkInterfaceProperties"
                    && path == "/eth_test/technologyType"
                    && *data == AstarteType::String("Ethernet".to_string())
            })
            .returning(|_, _, _| Ok(()));

        client
            .expect_send()
            .times(1)
            .in_sequence(&mut seq)
            .withf(|interface, path, data| {
                interface == "io.edgehog.devicemanager.NetworkInterfaceProperties"
                    && path == "/cellular_test/macAddress"
                    && *data == AstarteType::String("22:33:44:55:66:77".to_string())
            })
            .returning(|_, _, _| Ok(()));

        client
            .expect_send()
            .times(1)
            .in_sequence(&mut seq)
            .withf(|interface, path, data| {
                interface == "io.edgehog.devicemanager.NetworkInterfaceProperties"
                    && path == "/cellular_test/technologyType"
                    && *data == AstarteType::String("Cellular".to_string())
            })
            .returning(|_, _, _| Ok(()));

        for nt_if in eth_wifi {
            nt_if.send(&client).await;
        }
    }

    #[tokio::test]
    async fn get_supported_network_interfaces_run_test() {
        let mut client = MockPubSub::new();

        client
            .expect_send()
            .times(..)
            .withf(|interface, path, data| {
                interface == "io.edgehog.devicemanager.NetworkInterfaceProperties"
                    && path.ends_with("/macAddress")
                    && matches!(data, AstarteType::String(_))
            })
            .returning(|_, _, _| Ok(()));

        client
            .expect_send()
            .times(..)
            .withf(|interface, path, data| {
                interface == "io.edgehog.devicemanager.NetworkInterfaceProperties"
                    && path.ends_with("/technologyType")
                    && matches!(data, AstarteType::String(_))
            })
            .returning(|_, _, _| Ok(()));

        send_network_interface_properties(&client).await;
    }

    #[test]
    fn should_get_net_devices() {
        assert!(net_devices().is_ok());
    }
}
