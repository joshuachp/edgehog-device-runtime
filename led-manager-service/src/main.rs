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

use log::{debug, info, LevelFilter};
use zbus::{dbus_interface, ConnectionBuilder};

pub const SERVICE_NAME: &str = "io.edgehog.LedManager";

#[derive(Debug)]
struct LedManager {
    leds: Vec<String>,
}

#[dbus_interface(name = "io.edgehog.LedManager1")]
impl LedManager {
    fn list(&self) -> Vec<String> {
        debug!("listing {} leds", self.leds.len());

        self.leds.clone()
    }

    fn insert(&mut self, id: String) {
        debug!("adding led {id}");

        self.leds.push(id);
    }

    fn set(&self, id: String, status: bool) -> bool {
        const RESULT: bool = true;

        info!("SET {} -> {}: result {}", id, status, RESULT);

        RESULT
    }
}

#[tokio::main]
async fn main() -> stable_eyre::Result<()> {
    stable_eyre::install()?;
    env_logger::builder()
        .filter_level(LevelFilter::Info)
        .try_init()?;

    let leds = LedManager { leds: Vec::new() };

    let _conn = ConnectionBuilder::session()?
        .name(SERVICE_NAME)?
        .serve_at("/io/edgehog/LedManager", leds)?
        .build()
        .await?;

    info!("Service {SERVICE_NAME} started");

    tokio::signal::ctrl_c().await?;

    Ok(())
}
