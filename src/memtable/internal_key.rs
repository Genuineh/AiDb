//! # Internal Key Format
//!
//! This module defines the internal key format used in the MemTable and SSTable.
//!
//! ## Format
//!
//! ```text
//! InternalKey:
//!   [user_key: bytes] [sequence: u64] [type: u8]
//! ```
//!
//! ## Ordering
//!
//! InternalKeys are ordered by:
//! 1. user_key (ascending)
//! 2. sequence (descending - newer first)
//! 3. type (descending - Value before Deletion)

use std::cmp::Ordering;

/// The type of a value in the database.
///
/// - `Value`: A normal key-value pair
/// - `Deletion`: A tombstone marking that a key has been deleted
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ValueType {
    /// A tombstone indicating the key has been deleted
    Deletion = 0,
    
    /// A normal value
    Value = 1,
}

impl ValueType {
    /// Converts a u8 to a ValueType.
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(ValueType::Deletion),
            1 => Some(ValueType::Value),
            _ => None,
        }
    }

    /// Converts the ValueType to a u8.
    pub fn as_u8(self) -> u8 {
        self as u8
    }
}

/// Internal key used in MemTable and SSTable.
///
/// The internal key consists of:
/// - User key: The key provided by the user
/// - Sequence number: A monotonically increasing number for MVCC
/// - Value type: Either Value or Deletion (tombstone)
///
/// # Ordering
///
/// InternalKeys are ordered by:
/// 1. User key (ascending)
/// 2. Sequence number (descending - newer versions first)
/// 3. Value type (descending - Value before Deletion)
///
/// This ordering ensures that:
/// - Keys are sorted lexicographically
/// - The most recent version of a key appears first
/// - Values appear before deletions for the same key and sequence
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InternalKey {
    user_key: Vec<u8>,
    sequence: u64,
    value_type: ValueType,
}

impl InternalKey {
    /// Creates a new InternalKey.
    ///
    /// # Arguments
    ///
    /// * `user_key` - The user-provided key
    /// * `sequence` - The sequence number for this operation
    /// * `value_type` - The type of value (Value or Deletion)
    ///
    /// # Example
    ///
    /// ```rust
    /// use aidb::memtable::{InternalKey, ValueType};
    ///
    /// let key = InternalKey::new(b"user_key".to_vec(), 42, ValueType::Value);
    /// ```
    pub fn new(user_key: Vec<u8>, sequence: u64, value_type: ValueType) -> Self {
        Self {
            user_key,
            sequence,
            value_type,
        }
    }

    /// Returns the user key.
    pub fn user_key(&self) -> &[u8] {
        &self.user_key
    }

    /// Returns the sequence number.
    pub fn sequence(&self) -> u64 {
        self.sequence
    }

    /// Returns the value type.
    pub fn value_type(&self) -> ValueType {
        self.value_type
    }

    /// Encodes the InternalKey into bytes.
    ///
    /// Format: [user_key][sequence: 8 bytes][type: 1 byte]
    pub fn encode(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(self.user_key.len() + 9);
        buf.extend_from_slice(&self.user_key);
        buf.extend_from_slice(&self.sequence.to_le_bytes());
        buf.push(self.value_type.as_u8());
        buf
    }

    /// Decodes an InternalKey from bytes.
    ///
    /// # Errors
    ///
    /// Returns None if the data is too short or the value type is invalid.
    pub fn decode(data: &[u8]) -> Option<Self> {
        if data.len() < 9 {
            return None;
        }

        let user_key_len = data.len() - 9;
        let user_key = data[..user_key_len].to_vec();
        
        let sequence_bytes: [u8; 8] = data[user_key_len..user_key_len + 8]
            .try_into()
            .ok()?;
        let sequence = u64::from_le_bytes(sequence_bytes);
        
        let value_type = ValueType::from_u8(data[user_key_len + 8])?;

        Some(Self {
            user_key,
            sequence,
            value_type,
        })
    }

    /// Returns the total encoded size of this InternalKey.
    pub fn encoded_size(&self) -> usize {
        self.user_key.len() + 9
    }
}

impl PartialOrd for InternalKey {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for InternalKey {
    fn cmp(&self, other: &Self) -> Ordering {
        // First, compare user keys (ascending)
        match self.user_key.cmp(&other.user_key) {
            Ordering::Equal => {
                // If user keys are equal, compare sequence numbers (descending)
                match other.sequence.cmp(&self.sequence) {
                    Ordering::Equal => {
                        // If sequences are equal, compare types (descending)
                        other.value_type.cmp(&self.value_type)
                    }
                    other_ordering => other_ordering,
                }
            }
            other_ordering => other_ordering,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_value_type_conversion() {
        assert_eq!(ValueType::Deletion.as_u8(), 0);
        assert_eq!(ValueType::Value.as_u8(), 1);
        
        assert_eq!(ValueType::from_u8(0), Some(ValueType::Deletion));
        assert_eq!(ValueType::from_u8(1), Some(ValueType::Value));
        assert_eq!(ValueType::from_u8(2), None);
    }

    #[test]
    fn test_internal_key_creation() {
        let key = InternalKey::new(b"test_key".to_vec(), 42, ValueType::Value);
        
        assert_eq!(key.user_key(), b"test_key");
        assert_eq!(key.sequence(), 42);
        assert_eq!(key.value_type(), ValueType::Value);
    }

    #[test]
    fn test_internal_key_encode_decode() {
        let original = InternalKey::new(b"test_key".to_vec(), 12345, ValueType::Value);
        let encoded = original.encode();
        let decoded = InternalKey::decode(&encoded).unwrap();
        
        assert_eq!(decoded.user_key(), original.user_key());
        assert_eq!(decoded.sequence(), original.sequence());
        assert_eq!(decoded.value_type(), original.value_type());
    }

    #[test]
    fn test_internal_key_decode_invalid() {
        // Too short
        assert!(InternalKey::decode(&[1, 2, 3]).is_none());
        
        // Invalid value type
        let mut buf = vec![1, 2, 3, 4, 5, 6, 7, 8]; // 8 bytes
        buf.extend_from_slice(&42u64.to_le_bytes());
        buf.push(99); // Invalid type
        assert!(InternalKey::decode(&buf).is_none());
    }

    #[test]
    fn test_internal_key_ordering_by_user_key() {
        let key1 = InternalKey::new(b"a".to_vec(), 100, ValueType::Value);
        let key2 = InternalKey::new(b"b".to_vec(), 100, ValueType::Value);
        
        assert!(key1 < key2);
        assert!(key2 > key1);
    }

    #[test]
    fn test_internal_key_ordering_by_sequence() {
        // Same user key, different sequences (newer first)
        let key1 = InternalKey::new(b"key".to_vec(), 100, ValueType::Value);
        let key2 = InternalKey::new(b"key".to_vec(), 50, ValueType::Value);
        
        // Higher sequence should come first (descending order)
        assert!(key1 < key2);
    }

    #[test]
    fn test_internal_key_ordering_by_type() {
        // Same user key and sequence, different types
        let value_key = InternalKey::new(b"key".to_vec(), 100, ValueType::Value);
        let delete_key = InternalKey::new(b"key".to_vec(), 100, ValueType::Deletion);
        
        // Value should come before Deletion
        assert!(value_key < delete_key);
    }

    #[test]
    fn test_internal_key_complete_ordering() {
        let mut keys = [
            InternalKey::new(b"key2".to_vec(), 100, ValueType::Value),
            InternalKey::new(b"key1".to_vec(), 50, ValueType::Value),
            InternalKey::new(b"key1".to_vec(), 100, ValueType::Deletion),
            InternalKey::new(b"key1".to_vec(), 100, ValueType::Value),
            InternalKey::new(b"key1".to_vec(), 150, ValueType::Value),
        ];

        keys.sort();

        // Expected order:
        // 1. key1, seq=150, Value
        // 2. key1, seq=100, Value
        // 3. key1, seq=100, Deletion
        // 4. key1, seq=50, Value
        // 5. key2, seq=100, Value

        assert_eq!(keys[0].user_key(), b"key1");
        assert_eq!(keys[0].sequence(), 150);
        
        assert_eq!(keys[1].user_key(), b"key1");
        assert_eq!(keys[1].sequence(), 100);
        assert_eq!(keys[1].value_type(), ValueType::Value);
        
        assert_eq!(keys[2].user_key(), b"key1");
        assert_eq!(keys[2].sequence(), 100);
        assert_eq!(keys[2].value_type(), ValueType::Deletion);
        
        assert_eq!(keys[3].user_key(), b"key1");
        assert_eq!(keys[3].sequence(), 50);
        
        assert_eq!(keys[4].user_key(), b"key2");
    }

    #[test]
    fn test_encoded_size() {
        let key = InternalKey::new(b"test".to_vec(), 100, ValueType::Value);
        assert_eq!(key.encoded_size(), 4 + 8 + 1); // user_key(4) + sequence(8) + type(1)
    }
}
