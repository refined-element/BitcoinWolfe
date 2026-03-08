use std::sync::Arc;

use lightning::io;
use lightning::util::persist::KVStoreSync;
use redb::{Database, TableDefinition};

/// Table storing all LDK persistence data.
/// Key format: "namespace/secondary_namespace/key" -> raw bytes
const LN_KV: TableDefinition<&str, &[u8]> = TableDefinition::new("ln_kv");

/// A KVStoreSync implementation backed by redb.
///
/// LDK uses namespaced keys for all its persistence:
/// - `"channel_monitors"` / `""` / `"<outpoint>"` for channel monitors
/// - `"channel_manager"` / `""` / `"manager"` for the channel manager
/// - `"network_graph"` / `""` / `"graph"` for the network graph
/// - `"scorer"` / `""` / `"scorer"` for the scorer
///
/// We concatenate these into a single redb table key.
pub struct WolfeKVStore {
    db: Arc<Database>,
}

impl WolfeKVStore {
    pub fn new(db: Arc<Database>) -> Self {
        // Ensure the table exists
        if let Ok(write_txn) = db.begin_write() {
            let _ = write_txn.open_table(LN_KV);
            let _ = write_txn.commit();
        }
        Self { db }
    }

    fn make_key(primary_namespace: &str, secondary_namespace: &str, key: &str) -> String {
        if secondary_namespace.is_empty() {
            format!("{}/{}", primary_namespace, key)
        } else {
            format!("{}/{}/{}", primary_namespace, secondary_namespace, key)
        }
    }

    fn make_prefix(primary_namespace: &str, secondary_namespace: &str) -> String {
        if secondary_namespace.is_empty() {
            format!("{}/", primary_namespace)
        } else {
            format!("{}/{}/", primary_namespace, secondary_namespace)
        }
    }
}

impl KVStoreSync for WolfeKVStore {
    fn read(
        &self,
        primary_namespace: &str,
        secondary_namespace: &str,
        key: &str,
    ) -> Result<Vec<u8>, io::Error> {
        let db_key = Self::make_key(primary_namespace, secondary_namespace, key);

        let read_txn = self
            .db
            .begin_read()
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;

        let table = read_txn
            .open_table(LN_KV)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;

        match table.get(db_key.as_str()) {
            Ok(Some(guard)) => Ok(guard.value().to_vec()),
            Ok(None) => Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("key not found: {}", db_key),
            )),
            Err(e) => Err(io::Error::new(io::ErrorKind::Other, e.to_string())),
        }
    }

    fn write(
        &self,
        primary_namespace: &str,
        secondary_namespace: &str,
        key: &str,
        buf: Vec<u8>,
    ) -> Result<(), io::Error> {
        let db_key = Self::make_key(primary_namespace, secondary_namespace, key);

        let write_txn = self
            .db
            .begin_write()
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;

        {
            let mut table = write_txn
                .open_table(LN_KV)
                .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;

            table
                .insert(db_key.as_str(), buf.as_slice())
                .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;
        }

        write_txn
            .commit()
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;

        Ok(())
    }

    fn remove(
        &self,
        primary_namespace: &str,
        secondary_namespace: &str,
        key: &str,
        _lazy: bool,
    ) -> Result<(), io::Error> {
        let db_key = Self::make_key(primary_namespace, secondary_namespace, key);

        let write_txn = self
            .db
            .begin_write()
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;

        {
            let mut table = write_txn
                .open_table(LN_KV)
                .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;

            table
                .remove(db_key.as_str())
                .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;
        }

        write_txn
            .commit()
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;

        Ok(())
    }

    fn list(
        &self,
        primary_namespace: &str,
        secondary_namespace: &str,
    ) -> Result<Vec<String>, io::Error> {
        let prefix = Self::make_prefix(primary_namespace, secondary_namespace);

        // Compute the exclusive upper bound for the range query.
        // Since our keys are ASCII, incrementing the last byte of the prefix
        // gives us an exclusive end bound that captures all prefixed keys.
        let range_end = {
            let mut end = prefix.clone();
            // Replace trailing '/' with '0' (next ASCII char after '/') to form exclusive bound
            // '/' is 0x2F, '0' is 0x30
            if let Some(last) = end.pop() {
                end.push((last as u8 + 1) as char);
            }
            end
        };

        let read_txn = self
            .db
            .begin_read()
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;

        let table = read_txn
            .open_table(LN_KV)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;

        let mut keys = Vec::new();
        let iter = table
            .range(prefix.as_str()..range_end.as_str())
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;

        for entry in iter {
            let (key_guard, _) =
                entry.map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;
            let full_key = key_guard.value();
            if full_key.starts_with(&prefix) {
                let short_key = &full_key[prefix.len()..];
                keys.push(short_key.to_string());
            }
        }

        Ok(keys)
    }
}
