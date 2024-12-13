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

use std::{fmt::Debug, path::PathBuf, sync::Arc};

use migrations::run_migrations;
use rusqlite::{Connection, OpenFlags, Transaction};
use tokio::{sync::Mutex, task::JoinError};
use tracing::warn;

mod migrations;
mod queries;

/// Result for the [`HandleError`] returned by the [`Handle`].
pub type Result<T> = std::result::Result<T, HandleError>;

macro_rules! include_query {
    ($file:expr) => {
        include_str!(concat!("../../queries/", $file))
    };
}

pub(self) use include_query;

/// Handler error
#[derive(Debug, thiserror::Error, displaydoc::Display)]
pub enum HandleError {
    /// couldn't open database {path}
    Open {
        /// Path to the database we tried to open
        path: PathBuf,
        /// The SQLite error
        #[source]
        backtrace: rusqlite::Error,
    },
    /// couldn't spawned join task
    Join(#[from] JoinError),
    /// couldn't update PRAGMA value
    Pragma(#[source] rusqlite::Error),
    /// couldn't run migration
    Migration {
        /// Name of the migration that failed
        name: &'static str,
        #[source]
        /// The SQLite error
        backtrace: rusqlite::Error,
    },
    /// couldn't run transaction
    Transaction(#[source] rusqlite::Error),
    /// couldn't execute query
    Query(#[from] rusqlite::Error),
}

/// Read and write connection to the database
pub struct Handle {
    db_file: PathBuf,
    /// Write handle to the database
    pub writer: Arc<Mutex<Connection>>,
    /// Per task/thread reader
    // NOTE: this is needed because the connection isn't Sync, and we need to pass the Connection
    //       to another thread (for tokio). The option signal if the connection was invalidated by
    //       the inner task panicking. In that case we re-create the reader connection.
    pub reader: Option<Box<Connection>>,
}

impl Handle {
    /// Create a new instance by connecting to the file
    pub async fn open(db_file: PathBuf) -> Result<Self> {
        let writer = Self::connect(db_file.clone(), true).await?;

        let writer = Arc::new(Mutex::new(writer));
        let reader = Self::connect(db_file.clone(), false).await?;

        Ok(Self {
            db_file,
            writer,
            reader: Some(Box::new(reader)),
        })
    }

    /// Sets options for the connection
    async fn connect(db_file: PathBuf, writer: bool) -> Result<Connection> {
        let mut flags = OpenFlags::SQLITE_OPEN_NO_MUTEX;

        if writer {
            flags |= OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE;
        } else {
            flags |= OpenFlags::SQLITE_OPEN_READ_ONLY;
        }

        tokio::task::spawn_blocking(move || {
            let mut connection =
                Connection::open_with_flags(&db_file, flags).map_err(|err| HandleError::Open {
                    path: db_file,
                    backtrace: err,
                })?;

            // Persistent PRAGMA
            if writer {
                connection
                    .pragma_update(None, "journal_mode", "WALL")
                    .map_err(HandleError::Pragma)?;
                connection
                    .pragma_update(None, "synchronous", "NORMAL")
                    .map_err(HandleError::Pragma)?;
                // 64 MB
                connection
                    .pragma_update(None, "journal_size_limit", 67108864)
                    .map_err(HandleError::Pragma)?;
                // Reduce the size of the database
                connection
                    .pragma_update(None, "auto_vacuum", "INCREMENTAL")
                    .map_err(HandleError::Pragma)?;

                run_migrations(&mut connection)?;
            }

            connection
                .pragma_update(None, "cache_size", 2000)
                .map_err(HandleError::Pragma)?;
            // 5sec
            connection
                .pragma_update(None, "busy_timeout", 5000)
                .map_err(HandleError::Pragma)?;

            Ok(connection)
        })
        .await?
    }

    /// Create a new handle for the store
    pub async fn clone_handle(&self) -> Result<Self> {
        let reader = Self::connect(self.db_file.clone(), false).await?;

        Ok(Self {
            db_file: self.db_file.clone(),
            writer: Arc::clone(&self.writer),
            reader: Some(Box::new(reader)),
        })
    }

    /// Passes the reader to a callback to execute a query.
    pub async fn for_read<F, O>(&mut self, f: F) -> Result<O>
    where
        F: FnOnce(&mut Connection) -> Result<O> + Send + 'static,
        O: Send + 'static,
    {
        // Take
        let mut reader = match self.reader.take() {
            Some(reader) => reader,
            None => {
                warn!(
                    "connection missing task probably panicked, establishing a new one to {}",
                    self.db_file.display()
                );

                Self::connect(self.db_file.clone(), false)
                    .await
                    .map(Box::new)?
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
        F: FnOnce(&mut Connection) -> Result<O> + Send + 'static,
        O: Send + 'static,
    {
        let mut writer = Arc::clone(&self.writer).lock_owned().await;

        tokio::task::spawn_blocking(move || (f)(&mut writer)).await?
    }

    /// Passes the writer to a callback with a transaction already started.
    pub async fn for_write_transaction<F, O>(&self, f: F) -> Result<O>
    where
        F: FnOnce(&mut Transaction<'_>) -> Result<O> + Send + 'static,
        O: Send + 'static,
    {
        let mut writer = Arc::clone(&self.writer).lock_owned().await;

        tokio::task::spawn_blocking(move || {
            let mut transaction = writer.transaction().map_err(HandleError::Transaction)?;

            let out = (f)(&mut transaction)?;

            transaction.commit().map_err(HandleError::Transaction)?;

            Ok(out)
        })
        .await?
    }
}

impl Debug for Handle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Store")
            .field("db_file", &self.db_file)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;

    #[tokio::test]
    async fn should_open_db() {
        let tmp = TempDir::with_prefix("should_open").unwrap();

        Handle::open(tmp.path().join("database.db")).await.unwrap();
    }
}
