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

//! Structure to handle the SQLite store.
//!
//! ## Concurrency
//!
//! It handles concurrency by having a shared Mutex for the writer part and a per instance reader.
//! To have a new reader you need to open a new connection to the database.
//!
//! We pass a mutable reference to the connection to a [`FnOnce`]. If the closure panics the
//! connection will be lost and needs to be recreated.

use std::{error::Error, fmt::Debug, sync::Arc};

use diesel::{Connection, ConnectionError, SqliteConnection};
use diesel_migrations::MigrationHarness;
use tokio::{sync::Mutex, task::JoinError};
use tracing::warn;

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
    /// couldn't execute the query
    Query(#[from] diesel::result::Error),
    /// couldn't run pending migrations
    Migrations(#[source] DynError),
}

/// Read and write connection to the database
pub struct Handle {
    db_file: String,
    /// Write handle to the database
    pub writer: Arc<Mutex<SqliteConnection>>,
    /// Per task/thread reader
    // NOTE: this is needed because the connection isn't Sync, and we need to pass the Connection
    //       to another thread (for tokio). The option signal if the connection was invalidated by
    //       the inner task panicking. In that case we re-create the reader connection.
    pub reader: Option<Box<SqliteConnection>>,
}

impl Handle {
    /// Create a new instance by connecting to the file
    pub async fn open(db_file: &str) -> Result<Self> {
        let mut writer = Self::establish(db_file)?;

        let writer = tokio::task::spawn_blocking(move || -> Result<SqliteConnection> {
            writer
                .run_pending_migrations(MIGRATIONS)
                .map_err(HandleError::Migrations)?;

            Ok(writer)
        })
        .await??;

        let writer = Arc::new(Mutex::new(writer));
        let reader = Self::establish(db_file)?;

        Ok(Self {
            db_file: db_file.to_string(),
            writer,
            reader: Some(Box::new(reader)),
        })
    }

    /// Sets options for the connection
    fn establish(db_file: &str) -> Result<SqliteConnection> {
        SqliteConnection::establish(db_file).map_err(|err| HandleError::Connection {
            db_file: db_file.to_string(),
            backtrace: err,
        })
    }

    /// Create a new handle for the store
    pub fn clone_handle(&self) -> Result<Self> {
        let reader = Self::establish(&self.db_file)?;

        Ok(Self {
            db_file: self.db_file.clone(),
            writer: Arc::clone(&self.writer),
            reader: Some(Box::new(reader)),
        })
    }

    /// Passes the reader to a callback to execute a query.
    pub async fn for_read<F, O>(&mut self, f: F) -> Result<O>
    where
        F: FnOnce(&mut SqliteConnection) -> Result<O> + Send + 'static,
        O: Send + 'static,
    {
        // Take
        let mut reader = match self.reader.take() {
            Some(reader) => reader,
            None => {
                warn!(
                    "connection missing task probably panicked, establishing a new one to {}",
                    self.db_file
                );

                Self::establish(&self.db_file).map(Box::new)?
            }
        };

        // If this task panics (the error is returned) the connection would still be null
        let (reader, res) = tokio::task::spawn_blocking(move || {
            let res = (f)(&mut reader);

            (reader, res)
        })
        .await?;

        self.reader = Some(reader);

        res
    }

    /// Passes the writer to a callback to execute an insert, update or delete.
    pub async fn for_write<F, O>(&self, f: F) -> Result<O>
    where
        F: FnOnce(&mut SqliteConnection) -> Result<O> + Send + 'static,
        O: Send + 'static,
    {
        let mut writer = Arc::clone(&self.writer).lock_owned().await;

        tokio::task::spawn_blocking(move || (f)(&mut writer)).await?
    }
}

impl Debug for Handle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Store")
            .field("db_file", &self.db_file)
            .finish_non_exhaustive()
    }
}
