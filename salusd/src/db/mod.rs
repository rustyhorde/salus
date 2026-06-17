// Copyright (c) 2025 salus developers
//
// Licensed under the Apache License, Version 2.0
// <LICENSE-APACHE or https://www.apache.org/licenses/LICENSE-2.0> or the MIT
// license <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. All files in the project carrying such notice may not be copied,
// modified, or distributed except according to those terms.

use std::{
    borrow::Borrow,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use anyhow::{Context, Result};
use redb::{AccessGuard, Database, DatabaseError, Key, ReadableDatabase, TableDefinition, Value};

use crate::{
    config::PathDefaults,
    db::values::{config::ConfigVal, salus::SalusVal},
    error::Error,
    utils::{ensure_parent_dir, to_path_buf},
};

pub(crate) mod values;

pub(crate) const SALUS_CONFIG_TABLE_DEF: TableDefinition<'_, &str, ConfigVal> =
    TableDefinition::new("salus_config");

pub(crate) const SALUS_VAL_TABLE_DEF: TableDefinition<'_, String, SalusVal> =
    TableDefinition::new("salus_store");
pub(crate) const INITIALIZED_KEY: &str = "INITIALIZED";
pub(crate) const NUM_SHARES_KEY: &str = "NUM_SHARES";
pub(crate) const THRESHOLD_KEY: &str = "THRESHOLD";
pub(crate) const CHECK_KEY_KEY: &str = "CHECK_KEY";

pub(crate) fn initialize_redb<T: PathDefaults>(defaults: &T) -> Result<Arc<Mutex<Database>>> {
    let redb_path = database_absolute_path(defaults)?;
    ensure_parent_dir(&redb_path)?;
    let db = open_database(&redb_path)?;
    Ok(Arc::new(Mutex::new(db)))
}

/// Open (creating if needed) the redb database at `path`, mapping redb's
/// lock-contention error into an actionable [`Error::DatabaseLocked`] that names
/// the path and the likely cause (another `salusd` already running).
fn open_database(path: &Path) -> Result<Database> {
    match Database::create(path) {
        Ok(db) => Ok(db),
        Err(DatabaseError::DatabaseAlreadyOpen) => {
            Err(anyhow::Error::new(DatabaseError::DatabaseAlreadyOpen))
                .with_context(|| Error::DatabaseLocked(path.to_path_buf()))
        }
        // Known variants listed explicitly to satisfy `non_exhaustive_omitted_patterns`;
        // the trailing `Err(other)` forwards any future `#[non_exhaustive]` variant.
        Err(
            other @ (DatabaseError::RepairAborted
            | DatabaseError::UpgradeRequired(_)
            | DatabaseError::TransactionInProgress
            | DatabaseError::Storage(_))
            | other,
        ) => Err(anyhow::Error::new(other)),
    }
}

pub(crate) fn write_value<'a, K, V>(
    db: &mut Database,
    table_def: TableDefinition<'_, K, V>,
    key: K,
    value: V,
) -> Result<()>
where
    K: Key + Borrow<K::SelfType<'a>>,
    V: Value + Borrow<V::SelfType<'a>>,
{
    let write_txn = db.begin_write()?;
    {
        let mut table = write_txn.open_table(table_def)?;
        let _old_val = table.insert(key, value)?;
    }
    write_txn.commit()?;
    Ok(())
}

pub(crate) fn read_value<'a, K, V>(
    db: &Database,
    table_def: TableDefinition<'_, K, V>,
    key: K,
) -> Result<Option<AccessGuard<'a, V>>>
where
    K: Key + Borrow<K::SelfType<'a>>,
    V: Value<SelfType<'a> = V> + Borrow<V::SelfType<'a>>,
{
    let read_txn = db.begin_read()?;
    let table = read_txn.open_table(table_def)?;
    if let Some(value) = table.get(key)? {
        Ok(Some(value))
    } else {
        Ok(None)
    }
}

fn database_absolute_path<D>(defaults: &D) -> Result<PathBuf>
where
    D: PathDefaults,
{
    let default_fn = || -> Result<PathBuf> { default_database_absolute_path(defaults) };
    defaults
        .database_absolute_path()
        .as_ref()
        .map_or_else(default_fn, to_path_buf)
}

fn default_database_absolute_path<D>(defaults: &D) -> Result<PathBuf>
where
    D: PathDefaults,
{
    let base = dirs2::data_dir().ok_or(Error::DataDir)?;
    Ok(db_file_in(&base, &defaults.app_name()))
}

/// Compose the default database file path: `<base>/<app>/<app>.redb`.
fn db_file_in(base: &Path, app: &str) -> PathBuf {
    base.join(app).join(app).with_extension("redb")
}

pub(crate) fn unlock_redb(
    redb_s: &Arc<Mutex<Database>>,
    mut redb_fn: impl FnMut(&mut Database) -> Result<()>,
) -> Result<()> {
    let mut redb = match redb_s.lock() {
        Ok(share_store) => share_store,
        Err(poisoned) => poisoned.into_inner(),
    };
    redb_fn(&mut redb)
}

#[cfg(test)]
mod test {
    use std::{
        path::{Path, PathBuf},
        time::{SystemTime, UNIX_EPOCH},
    };

    use anyhow::{Result, bail};

    use super::{db_file_in, open_database};
    use crate::error::Error;

    #[test]
    fn db_file_in_composes_app_dir_and_extension() {
        let path = db_file_in(Path::new("/base"), "salusd");
        assert_eq!(path, Path::new("/base/salusd/salusd.redb"));
    }

    fn unique_db_path() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        let name = format!("salusd-test-{}-{nanos}.redb", std::process::id());
        std::env::temp_dir().join(name)
    }

    #[test]
    fn open_database_reports_lock_contention() -> Result<()> {
        let path = unique_db_path();
        // Hold the database open so the second open contends for the lock.
        let _first = open_database(&path)?;

        let result = open_database(&path);
        let cleanup = || {
            drop(std::fs::remove_file(&path));
        };

        let Err(err) = result else {
            cleanup();
            bail!(
                "expected lock contention error opening {} twice",
                path.display()
            );
        };

        let matched =
            matches!(err.downcast_ref::<Error>(), Some(Error::DatabaseLocked(p)) if p == &path);
        cleanup();
        if !matched {
            bail!(
                "expected Error::DatabaseLocked({}), got: {err:?}",
                path.display()
            );
        }
        Ok(())
    }
}
