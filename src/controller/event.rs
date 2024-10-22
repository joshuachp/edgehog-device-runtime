// This file is part of Edgehog.
//
// Copyright 2024 SECO Mind Srl
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//   http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.
//
// SPDX-License-Identifier: Apache-2.0

use astarte_device_sdk::{event::FromEventError, DeviceEvent, FromEvent};

use crate::{
    commands::Commands, led_behavior::LedEvent, ota::event::OtaRequest,
    telemetry::event::TelemetryEvent,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeEvent {
    Ota(OtaRequest),
    Command(Commands),
    Telemetry(TelemetryEvent),
    Led(LedEvent),
    #[cfg(feature = "forwarder")]
    Session(edgehog_forwarder::astarte::SessionInfo),
}

impl FromEvent for RuntimeEvent {
    type Err = FromEventError;

    fn from_event(event: DeviceEvent) -> Result<Self, Self::Err> {
        match event.interface.as_str() {
            "io.edgehog.devicemanager.OTARequest" => {
                OtaRequest::from_event(event).map(RuntimeEvent::Ota)
            }
            "io.edgehog.devicemanager.Commands" => {
                Commands::from_event(event).map(RuntimeEvent::Command)
            }
            "io.edgehog.devicemanager.config.Telemetry" => {
                TelemetryEvent::from_event(event).map(RuntimeEvent::Telemetry)
            }
            "io.edgehog.devicemanager.LedBehavior" => {
                LedEvent::from_event(event).map(RuntimeEvent::Led)
            }
            #[cfg(feature = "forwarder")]
            "io.edgehog.devicemanager.ForwarderSessionRequest" => {
                edgehog_forwarder::astarte::SessionInfo::from_event(event)
                    .map(RuntimeEvent::Session)
            }
            _ => Err(FromEventError::Interface(event.interface)),
        }
    }
}