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

use rusqlite::Connection;
use tracing::{instrument, trace};

use super::{include_query, HandleError, Result};

pub(super) struct Migration {
    pub(super) name: &'static str,
    pub(super) content: &'static str,
}

macro_rules! include_migration {
    ($file:expr) => {
        Migration {
            name: $file,
            content: include_str!(concat!("../../migrations/", $file)),
        }
    };
}

pub(super) const MIGRATION_TABLE: &str = include_str!("../../migrations/migrations.sql");

#[cfg(feature = "containers")]
pub(super) const CONTAINER_MIGRATIONS: &[Migration] =
    &[include_migration!("containers/0001-init.sql")];

#[instrument(skip_all)]
pub(super) fn run_migrations(connection: &mut Connection) -> Result<()> {
    connection
        .execute_batch(MIGRATION_TABLE)
        .map_err(|err| HandleError::Migration {
            name: "initial migrations",
            backtrace: err,
        })?;

    for migration in CONTAINER_MIGRATIONS {
        let transaction = connection.transaction().map_err(HandleError::Transaction)?;

        let exists: bool = transaction
            .query_row(
                include_query!("read/exists_migration.sql"),
                (migration.name,),
                |r| r.get::<usize, bool>(0),
            )
            .map_err(|err| HandleError::Migration {
                name: migration.name,
                backtrace: err,
            })?;

        if exists {
            trace!("migration {} already run", migration.name);

            continue;
        }

        transaction
            .execute_batch(&migration.content)
            .map_err(|err| HandleError::Migration {
                name: migration.name,
                backtrace: err,
            })?;

        transaction
            .prepare_cached(include_query!("write/insert_migration.sql"))
            .and_then(|mut st| st.execute((migration.name,)))
            .map_err(|err| HandleError::Migration {
                name: migration.name,
                backtrace: err,
            })?;

        transaction.commit().map_err(HandleError::Transaction)?;
    }

    Ok(())
}
