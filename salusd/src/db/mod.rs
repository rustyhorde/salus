// Copyright (c) 2025 salus developers
//
// Licensed under the Apache License, Version 2.0
// <LICENSE-APACHE or https://www.apache.org/licenses/LICENSE-2.0> or the MIT
// license <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. All files in the project carrying such notice may not be copied,
// modified, or distributed except according to those terms.

use std::{
    borrow::Borrow,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use anyhow::Result;
use redb::{AccessGuard, Database, Key, ReadableDatabase, TableDefinition, Value};

use crate::{
    config::PathDefaults,
    db::values::{config::ConfigVal, salus::SalusVal},
    utils::to_path_buf,
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
    let db = Database::create(redb_path)?;
    Ok(Arc::new(Mutex::new(db)))
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

#[allow(clippy::unnecessary_wraps)]
fn default_database_absolute_path<D>(defaults: &D) -> Result<PathBuf>
where
    D: PathDefaults,
{
    let mut database_file_path = PathBuf::from(defaults.default_database_path());
    database_file_path.push(defaults.default_database_file_name());
    let _ = database_file_path.set_extension("redb");
    Ok(database_file_path)
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
