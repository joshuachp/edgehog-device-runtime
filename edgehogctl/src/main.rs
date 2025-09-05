// This file is part of Edgehog.
//
// Copyright 2025 SECO Mind Srl
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//    http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.
//
// SPDX-License-Identifier: Apache-2.0

use std::io::{stdout, IsTerminal};

use clap::Parser;
use edgehogctl::cli::Cli;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> eyre::Result<()> {
    color_eyre::install()?;

    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer().with_ansi(stdout().is_terminal()))
        .with(
            EnvFilter::builder()
                .with_default_directive("edgehogctl=info".parse()?)
                .from_env_lossy(),
        )
        .try_init()?;

    let cli = Cli::parse();

    match cli.cmd {
        #[cfg(feature = "containers")]
        edgehogctl::cli::Cmd::Containers { cmd } => cmd.run().await?,
    }

    Ok(())
}
