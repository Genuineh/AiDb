//! WAL recovery entries → MemTable replay.

use crate::engine::memtable::MemTable;
use crate::engine::wal::record::{OpType, WalEntry};
use crate::error::{Error, Result};

pub fn apply_entry(mt: &MemTable, entry: &WalEntry) -> Result<()> {
    entry.validate()?;
    match entry.op_type {
        OpType::TypePut => {
            let value = entry
                .value
                .as_deref()
                .ok_or_else(|| Error::Corruption("put entry missing value".into()))?;
            mt.put(&entry.key, value, entry.sequence)
        }
        OpType::TypeDelete => mt.delete(&entry.key, entry.sequence),
        OpType::TypeDeleteRange => {
            let end = entry.value.as_deref().ok_or_else(|| {
                Error::Corruption("delete_range entry missing value".into())
            })?;
            mt.put_range_delete(&entry.key, end, entry.sequence)
        }
        OpType::BatchStart | OpType::FileHeader => Ok(()),
    }
}

pub fn replay_entries(mt: &MemTable, entries: &[WalEntry]) -> Result<()> {
    for entry in entries {
        apply_entry(mt, entry)?;
    }
    Ok(())
}
