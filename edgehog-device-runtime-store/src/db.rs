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

//! Structure to store the state of the docker service

use std::{
    error::Error,
    fmt::Debug,
    sync::{Arc, Mutex},
};

use diesel::{Connection, ConnectionError, SqliteConnection};
use diesel_migrations::MigrationHarness;
use tokio::task::JoinError;

use crate::schema::MIGRATIONS;

type DynError = Box<dyn Error + Send + Sync>;
type Result<T> = std::result::Result<T, HandleError>;

/// Handler error
#[derive(Debug, thiserror::Error, displaydoc::Display)]
pub enum HandleError {
    /// couldn't join database task
    Join(#[from] JoinError),
    /// couldn't connect to the database {db_file}
    Connection {
        /// Connection to the database file
        db_file: String,
        /// Underling connection error
        #[source]
        backtrace: ConnectionError,
    },
    /// couldn't run pending migrations
    Migrations(#[source] DynError),
}

/// Read and write connection to the database
pub struct Handle {
    db_file: String,
    /// Write handle to the database
    pub write: Arc<Mutex<SqliteConnection>>,
    /// Per task/thread reader
    // NOTE: consider using a pool of connection if needed by more threads.
    pub read: SqliteConnection,
}

impl Handle {
    /// Create a new instance by connecting to the file
    pub async fn open(db_file: &str) -> Result<Self> {
        let mut connection =
            SqliteConnection::establish(db_file).map_err(|err| HandleError::Connection {
                db_file: db_file.to_string(),
                backtrace: err,
            })?;

        let connection = tokio::task::spawn_blocking(move || -> Result<SqliteConnection> {
            connection
                .run_pending_migrations(MIGRATIONS)
                .map_err(HandleError::Migrations)?;

            Ok(connection)
        })
        .await??;

        let write = Arc::new(Mutex::new(connection));
        let read = SqliteConnection::establish(db_file).map_err(|err| HandleError::Connection {
            db_file: db_file.to_string(),
            backtrace: err,
        })?;

        Ok(Self {
            db_file: db_file.to_string(),
            write,
            read,
        })
    }

    /// Create a new handle for the store
    pub fn clone_handle(&self) -> Result<Self> {
        let read =
            SqliteConnection::establish(&self.db_file).map_err(|err| HandleError::Connection {
                db_file: self.db_file.to_string(),
                backtrace: err,
            })?;

        Ok(Self {
            db_file: self.db_file.clone(),
            write: Arc::clone(&self.write),
            read,
        })
    }
}

impl Debug for Handle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Store")
            .field("db_file", &self.db_file)
            .finish_non_exhaustive()
    }
}
