// This file is part of Edgehog.
//
// Copyright 2024 - 2025 SECO Mind Srl
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

//! Structure to handle the SQLite store.
//!
//! ## Concurrency
//!
//! It handles concurrency by having a shared Mutex for the writer part and a per instance reader.
//! To have a new reader you need to open a new connection to the database.
//!
//! We pass a mutable reference to the connection to a [`FnOnce`]. If the closure panics the
//! connection will be lost and needs to be recreated.

use std::{
    error::Error,
    fmt::Debug,
    num::NonZeroUsize,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use deadpool::unmanaged::Pool;
use diesel::{
    connection::SimpleConnection, sqlite::Sqlite, Connection, ConnectionError, SqliteConnection,
};
use sync_wrapper::SyncWrapper;
use tokio::{sync::Mutex, task::JoinError};
use tracing::debug;

type DynError = Box<dyn Error + Send + Sync>;
/// Result for the [`HandleError`] returned by the [`Handle`].
pub type Result<T> = std::result::Result<T, HandleError>;

/// PRAGMA for the connection
const INIT_READER: &str = include_str!("../assets/init-reader.sql");
const INIT_WRITER: &str = include_str!("../assets/init-writer.sql");

/// Handler error
#[derive(Debug, thiserror::Error, displaydoc::Display)]
pub enum HandleError {
    /// couldn't open database with non UTF-8 path: {0}
    NonUtf8Path(PathBuf),
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
    /// wrong number of rows updated, expected {exp} but modified {modified}
    UpdateRows {
        /// Number of rows modified.
        modified: usize,
        /// Expected number or rows.
        exp: usize,
    },
    /// error returned by the application
    #[error(transparent)]
    Application(DynError),
}

impl HandleError {
    /// Creates an [`HandleError::Application`] error.
    pub fn from_app(error: impl Into<DynError>) -> Self {
        Self::Application(error.into())
    }
}

impl HandleError {
    /// Check the result of the number of rows a query modified
    pub fn check_modified(modified: usize, exp: usize) -> Result<()> {
        if modified != exp {
            Err(HandleError::UpdateRows { exp, modified })
        } else {
            Ok(())
        }
    }
}

/// Read and write connection to the database
#[derive(Clone)]
pub struct Handle {
    pub db_file: Arc<str>,
    /// Write handle to the database
    pub writer: Arc<Mutex<SqliteConnection>>,
    /// Per task/thread reader
    // NOTE: this is needed because the connection isn't Sync, and we need to pass the Connection
    //       to another thread (for tokio). The option signal if the connection was invalidated by
    //       the inner task panicking. In that case we re-create the reader connection.
    pub reader: Pool<SqliteConnection>,
}

impl Handle {
    /// Create a new instance by connecting to the file
    pub async fn open(db_file: impl AsRef<Path>, options: SqliteOptios) -> Result<Self> {
        let db_path = db_file.as_ref();
        let db_str: Arc<str> = db_path
            .to_str()
            .ok_or_else(|| HandleError::NonUtf8Path(db_path.to_path_buf()))
            .map(Arc::from)?;

        let writer = options.establish(Arc::clone(&db_str), false).await?;
        // We don't have migrations other than the containers for now
        #[cfg(feature = "containers")]
        let mut writer = writer;

        let writer = tokio::task::spawn_blocking(move || -> Result<SqliteConnection> {
            #[cfg(feature = "containers")]
            {
                use diesel_migrations::MigrationHarness;
                writer
                    .run_pending_migrations(crate::schema::CONTAINER_MIGRATIONS)
                    .map_err(HandleError::Migrations)?;
            }

            Ok(writer)
        })
        .await??;

        Ok(Self {
            db_file: db_str,
            writer: Arc::new(Mutex::new(writer)),
            reader: Pool::new(options.max_pool_size.into()),
        })
    }

    /// Passes the reader to a callback to execute a query.
    pub async fn for_read<F, O>(&mut self, f: F) -> Result<O>
    where
        F: FnOnce(&mut SqliteConnection) -> Result<O> + Send + 'static,
        O: Send + 'static,
    {
        // Take the reader to move it to blocking task
        let mut reader = match self.reader.get_mut().take() {
            Some(reader) => reader,
            None => {
                debug!(
                    "connection missing, establishing a new one to {}",
                    self.db_file
                );

                Self::establish_reader(&self.db_file).await.map(Box::new)?
            }
        };

        // If this task panics (the error is returned) the connection would still be null
        let (reader, res) = tokio::task::spawn_blocking(move || {
            let res = (f)(&mut reader);

            (reader, res)
        })
        .await?;

        *self.reader.get_mut() = Some(reader);

        res
    }

    /// Passes the writer to a callback with a transaction already started.
    pub async fn for_write<F, O>(&self, f: F) -> Result<O>
    where
        F: FnOnce(&mut SqliteConnection) -> Result<O> + Send + 'static,
        O: Send + 'static,
    {
        let mut writer = Arc::clone(&self.writer).lock_owned().await;

        tokio::task::spawn_blocking(move || writer.transaction(|writer| (f)(writer))).await?
    }
}

impl Debug for Handle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Handle")
            .field("db_file", &self.db_file)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, Copy)]
struct SqliteOptios {
    max_pool_size: NonZeroUsize,
    busy_timout: Duration,
    cache_size: i16,
    max_page_count: u32,
    journal_size_limit: u64,
    wal_autocheckpoint: u32,
}

impl SqliteOptios {
    async fn establish(
        self,
        db_file: Arc<str>,
        reader: bool,
    ) -> std::result::Result<SqliteConnection, HandleError> {
        tokio::task::spawn_blocking(move || {
            let mut conn =
                SqliteConnection::establish(&db_file).map_err(|err| HandleError::Connection {
                    db_file: db_file.to_string(),
                    backtrace: err,
                })?;

            conn.batch_execute("PRAGMA journal_mode = wal;")?;
            conn.batch_execute("PRAGMA foreign_keys = true;")?;
            conn.batch_execute("PRAGMA synchronous = NORMAL;")?;
            conn.batch_execute("PRAGMA auto_vacuum = INCREMENTAL;")?;
            conn.batch_execute("PRAGMA temp_store = MEMORY;")?;
            // NOTE: Safe to format since we handle the options, do not pass strings.
            conn.batch_execute(&format!(
                "PRAGMA busy_timeout = {};",
                self.busy_timout.as_millis()
            ))?;
            conn.batch_execute(&format!("PRAGMA cache_size = {};", self.cache_size))?;
            conn.batch_execute(&format!("PRAGMA max_page_count = {};", self.max_page_count))?;
            conn.batch_execute(&format!(
                "PRAGMA journal_size_limit = {};",
                self.journal_size_limit
            ))?;
            conn.batch_execute(&format!(
                "PRAGMA wal_autocheckpoint = {};",
                self.wal_autocheckpoint
            ))?;

            if reader {
                conn.batch_execute("PRAGMA query_only = ON;")?;
            }

            Ok(conn)
        })
        .await?
    }
}

impl Default for SqliteOptios {
    fn default() -> Self {
        const DEFAULT_POOL_SIZE: NonZeroUsize = match NonZeroUsize::new(4) {
            Some(size) => size,
            None => unreachable!(),
        };

        Self {
            max_pool_size: std::thread::available_parallelism().unwrap_or(DEFAULT_POOL_SIZE),
            busy_timout: Duration::from_secs(5),
            // 2 kib
            cache_size: -2 * 1024,
            // 2 gib (assumes 4096 page size)
            max_page_count: 524288,
            // 64 mib
            journal_size_limit: 64 * 1024 * 1024,
            // 1000 pages
            wal_autocheckpoint: 1000,
        }
    }
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;

    #[tokio::test]
    async fn should_open_db() {
        let tmp = TempDir::with_prefix("should_open").unwrap();

        Handle::open(&tmp.path().join("database.db")).await.unwrap();
    }
}
