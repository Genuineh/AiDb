//! Bloom Filter implementation.
//!
//! A space-efficient probabilistic data structure used to test whether an element
//! is a member of a set. False positive matches are possible, but false negatives are not.

use crate::error::{Error, Result};
use crate::filter::Filter;
use std::hash::{Hash, Hasher};

/// Default bits per key for bloom filter
const DEFAULT_BITS_PER_KEY: usize = 10;

/// BloomFilter provides probabilistic set membership testing.
///
/// # Example
/// ```
/// use aidb::filter::{BloomFilter, Filter};
///
/// let mut filter = BloomFilter::new(1000, 0.01); // 1000 keys, 1% false positive rate
/// filter.add(b"key1");
/// filter.add(b"key2");
///
/// assert!(filter.may_contain(b"key1"));
/// assert!(filter.may_contain(b"key2"));
/// // key3 might return true (false positive) or false
/// ```
#[derive(Debug, Clone)]
pub struct BloomFilter {
    /// Bit array for the bloom filter
    bits: Vec<u8>,
    /// Number of hash functions to use
    num_hashes: u32,
    /// Number of bits in the filter
    num_bits: usize,
}

impl BloomFilter {
    /// Create a new BloomFilter with optimal parameters for expected number of keys and false positive rate.
    ///
    /// # Arguments
    /// * `expected_keys` - Expected number of keys to be inserted
    /// * `false_positive_rate` - Desired false positive rate (e.g., 0.01 for 1%)
    ///
    /// # Returns
    /// A new BloomFilter with optimal size and number of hash functions
    pub fn new(expected_keys: usize, false_positive_rate: f64) -> Self {
        if expected_keys == 0 {
            return Self::with_bits_and_hashes(64, 1);
        }
        
        // Calculate optimal number of bits: m = -n * ln(p) / (ln(2)^2)
        let num_bits = Self::optimal_num_bits(expected_keys, false_positive_rate);
        
        // Calculate optimal number of hash functions: k = (m/n) * ln(2)
        let num_hashes = Self::optimal_num_hashes(num_bits, expected_keys);
        
        Self::with_bits_and_hashes(num_bits, num_hashes)
    }
    
    /// Create a BloomFilter with a specific number of bits per key.
    ///
    /// This is useful when you want consistent sizing regardless of expected keys.
    ///
    /// # Arguments
    /// * `num_keys` - Expected number of keys
    /// * `bits_per_key` - Number of bits to allocate per key (default: 10)
    pub fn with_bits_per_key(num_keys: usize, bits_per_key: usize) -> Self {
        let num_bits = (num_keys * bits_per_key).max(64);
        let num_hashes = ((bits_per_key as f64) * 0.69).round() as u32; // 0.69 ≈ ln(2)
        let num_hashes = num_hashes.clamp(1, 30);
        
        Self::with_bits_and_hashes(num_bits, num_hashes)
    }
    
    /// Create a BloomFilter with default settings (10 bits per key).
    pub fn default_with_keys(num_keys: usize) -> Self {
        Self::with_bits_per_key(num_keys, DEFAULT_BITS_PER_KEY)
    }
    
    /// Create a BloomFilter with specific bits and hash count.
    fn with_bits_and_hashes(num_bits: usize, num_hashes: u32) -> Self {
        let num_bytes = (num_bits + 7) / 8;
        
        Self {
            bits: vec![0u8; num_bytes],
            num_hashes,
            num_bits,
        }
    }
    
    /// Calculate optimal number of bits for given parameters.
    fn optimal_num_bits(expected_keys: usize, false_positive_rate: f64) -> usize {
        let n = expected_keys as f64;
        let p = false_positive_rate.max(0.0001).min(0.9999); // Clamp to reasonable range
        
        let num_bits = (-n * p.ln() / (2.0_f64.ln().powi(2))).ceil() as usize;
        num_bits.max(64) // Minimum size
    }
    
    /// Calculate optimal number of hash functions.
    fn optimal_num_hashes(num_bits: usize, expected_keys: usize) -> u32 {
        if expected_keys == 0 {
            return 1;
        }
        
        let k = ((num_bits as f64 / expected_keys as f64) * 2.0_f64.ln()).ceil() as u32;
        k.clamp(1, 30) // Reasonable range for hash functions
    }
    
    /// Generate multiple hash values for a key using double hashing technique.
    ///
    /// We use two hash functions (hash1 and hash2) and generate k hashes as:
    /// hash_i = hash1 + i * hash2 (mod m)
    ///
    /// This is more efficient than computing k independent hash functions.
    fn hash_values(&self, key: &[u8]) -> Vec<usize> {
        // Use two different hash functions
        let hash1 = self.hash_with_seed(key, 0xbc9f1d34);
        let hash2 = self.hash_with_seed(key, 0xd0e89c7b);
        
        let mut hashes = Vec::with_capacity(self.num_hashes as usize);
        for i in 0..self.num_hashes {
            // Double hashing: h_i = h1 + i*h2
            let hash = hash1.wrapping_add(i.wrapping_mul(hash2));
            hashes.push((hash as usize) % self.num_bits);
        }
        
        hashes
    }
    
    /// Hash with a specific seed using a simple but effective hash function.
    ///
    /// This is based on the FNV-1a hash algorithm with modifications for better distribution.
    fn hash_with_seed(&self, key: &[u8], seed: u32) -> u32 {
        let mut hasher = FnvHasher::new_with_seed(seed);
        key.hash(&mut hasher);
        hasher.finish() as u32
    }
    
    /// Set a bit at the given position.
    fn set_bit(&mut self, pos: usize) {
        if pos < self.num_bits {
            self.bits[pos / 8] |= 1 << (pos % 8);
        }
    }
    
    /// Check if a bit is set at the given position.
    fn is_bit_set(&self, pos: usize) -> bool {
        if pos < self.num_bits {
            (self.bits[pos / 8] & (1 << (pos % 8))) != 0
        } else {
            false
        }
    }
    
    /// Get the size of the filter in bytes.
    pub fn size(&self) -> usize {
        self.bits.len()
    }
    
    /// Get the number of hash functions used.
    pub fn num_hashes(&self) -> u32 {
        self.num_hashes
    }
    
    /// Get the number of bits in the filter.
    pub fn num_bits(&self) -> usize {
        self.num_bits
    }
    
    /// Calculate the approximate false positive rate for the current state.
    ///
    /// This is an estimate based on the theoretical formula:
    /// p = (1 - e^(-kn/m))^k
    /// where k = num_hashes, n = num_keys (estimated), m = num_bits
    pub fn estimated_false_positive_rate(&self, num_keys: usize) -> f64 {
        if num_keys == 0 {
            return 0.0;
        }
        
        let k = self.num_hashes as f64;
        let n = num_keys as f64;
        let m = self.num_bits as f64;
        
        let exp = (-k * n / m).exp();
        (1.0 - exp).powf(k)
    }
}

impl Filter for BloomFilter {
    /// Check if a key may exist in the set.
    ///
    /// Returns `true` if the key might exist (with possible false positives).
    /// Returns `false` if the key definitely does not exist (no false negatives).
    fn may_contain(&self, key: &[u8]) -> bool {
        let hashes = self.hash_values(key);
        
        for hash in hashes {
            if !self.is_bit_set(hash) {
                return false; // Definitely not present
            }
        }
        
        true // Possibly present (or false positive)
    }
    
    /// Add a key to the filter.
    fn add(&mut self, key: &[u8]) {
        let hashes = self.hash_values(key);
        
        for hash in hashes {
            self.set_bit(hash);
        }
    }
    
    /// Encode the filter to bytes for storage.
    ///
    /// Format:
    /// [num_hashes: 4 bytes][num_bits: 8 bytes][bits: variable]
    fn encode(&self) -> Vec<u8> {
        let mut encoded = Vec::new();
        
        // Write num_hashes (4 bytes)
        encoded.extend_from_slice(&self.num_hashes.to_le_bytes());
        
        // Write num_bits (8 bytes)
        encoded.extend_from_slice(&(self.num_bits as u64).to_le_bytes());
        
        // Write bits
        encoded.extend_from_slice(&self.bits);
        
        encoded
    }
    
    /// Decode a filter from bytes.
    fn decode(data: &[u8]) -> Result<Self> {
        if data.len() < 12 {
            return Err(Error::corruption("Bloom filter data too short"));
        }
        
        // Read num_hashes (4 bytes)
        let num_hashes = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
        
        // Read num_bits (8 bytes)
        let num_bits = u64::from_le_bytes([
            data[4], data[5], data[6], data[7],
            data[8], data[9], data[10], data[11],
        ]) as usize;
        
        // Validate
        let expected_bytes = (num_bits + 7) / 8;
        if data.len() != 12 + expected_bytes {
            return Err(Error::corruption("Bloom filter size mismatch"));
        }
        
        // Read bits
        let bits = data[12..].to_vec();
        
        Ok(Self {
            bits,
            num_hashes,
            num_bits,
        })
    }
}

/// Simple FNV-1a hasher for Bloom Filter
struct FnvHasher {
    state: u64,
}

impl FnvHasher {
    const FNV_OFFSET_BASIS: u64 = 0xcbf29ce484222325;
    const FNV_PRIME: u64 = 0x100000001b3;
    
    fn new_with_seed(seed: u32) -> Self {
        Self {
            state: Self::FNV_OFFSET_BASIS ^ (seed as u64),
        }
    }
}

impl Hasher for FnvHasher {
    fn finish(&self) -> u64 {
        self.state
    }
    
    fn write(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.state ^= byte as u64;
            self.state = self.state.wrapping_mul(Self::FNV_PRIME);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_bloom_filter_basic() {
        let mut filter = BloomFilter::new(100, 0.01);
        
        // Add some keys
        filter.add(b"key1");
        filter.add(b"key2");
        filter.add(b"key3");
        
        // Check membership
        assert!(filter.may_contain(b"key1"));
        assert!(filter.may_contain(b"key2"));
        assert!(filter.may_contain(b"key3"));
        
        // These should likely return false (but could be false positives)
        // We don't assert false here because of potential false positives
    }
    
    #[test]
    fn test_bloom_filter_no_false_negatives() {
        let mut filter = BloomFilter::new(1000, 0.01);
        
        // Add many keys
        let keys: Vec<Vec<u8>> = (0..1000)
            .map(|i| format!("key{}", i).into_bytes())
            .collect();
        
        for key in &keys {
            filter.add(key);
        }
        
        // All added keys should be found (no false negatives)
        for key in &keys {
            assert!(
                filter.may_contain(key),
                "False negative detected for key: {:?}",
                String::from_utf8_lossy(key)
            );
        }
    }
    
    #[test]
    fn test_bloom_filter_false_positive_rate() {
        let num_keys = 10000;
        let target_fp_rate = 0.01; // 1%
        
        let mut filter = BloomFilter::new(num_keys, target_fp_rate);
        
        // Add keys
        for i in 0..num_keys {
            let key = format!("key{}", i);
            filter.add(key.as_bytes());
        }
        
        // Test with non-existent keys
        let test_keys = 10000;
        let mut false_positives = 0;
        
        for i in num_keys..(num_keys + test_keys) {
            let key = format!("key{}", i);
            if filter.may_contain(key.as_bytes()) {
                false_positives += 1;
            }
        }
        
        let actual_fp_rate = false_positives as f64 / test_keys as f64;
        
        println!("Target FP rate: {:.4}", target_fp_rate);
        println!("Actual FP rate: {:.4}", actual_fp_rate);
        println!("False positives: {}/{}", false_positives, test_keys);
        
        // Allow some margin (2x the target rate) due to randomness
        assert!(
            actual_fp_rate < target_fp_rate * 2.0,
            "False positive rate too high: {:.4} (expected < {:.4})",
            actual_fp_rate,
            target_fp_rate * 2.0
        );
    }
    
    #[test]
    fn test_bloom_filter_encode_decode() {
        let mut filter = BloomFilter::new(100, 0.01);
        
        // Add some keys
        filter.add(b"key1");
        filter.add(b"key2");
        filter.add(b"key3");
        
        // Encode
        let encoded = filter.encode();
        
        // Decode
        let decoded = BloomFilter::decode(&encoded).unwrap();
        
        // Verify decoded filter works the same
        assert!(decoded.may_contain(b"key1"));
        assert!(decoded.may_contain(b"key2"));
        assert!(decoded.may_contain(b"key3"));
        
        assert_eq!(filter.num_hashes(), decoded.num_hashes());
        assert_eq!(filter.num_bits(), decoded.num_bits());
    }
    
    #[test]
    fn test_bloom_filter_with_bits_per_key() {
        let filter = BloomFilter::with_bits_per_key(100, 10);
        
        assert!(filter.num_bits() >= 100 * 10);
        assert!(filter.num_hashes() > 0);
    }
    
    #[test]
    fn test_bloom_filter_empty() {
        let filter = BloomFilter::new(0, 0.01);
        
        // Empty filter should return false for any key
        assert!(!filter.may_contain(b"key1"));
    }
    
    #[test]
    fn test_bloom_filter_size() {
        let filter = BloomFilter::new(1000, 0.01);
        
        println!("Filter size: {} bytes", filter.size());
        println!("Num hashes: {}", filter.num_hashes());
        println!("Num bits: {}", filter.num_bits());
        
        assert!(filter.size() > 0);
        assert!(filter.num_hashes() > 0);
    }
    
    #[test]
    fn test_bloom_filter_estimated_fp_rate() {
        let num_keys = 1000;
        let filter = BloomFilter::new(num_keys, 0.01);
        
        let estimated = filter.estimated_false_positive_rate(num_keys);
        
        println!("Estimated FP rate: {:.6}", estimated);
        
        // Should be close to target rate
        assert!(estimated < 0.02); // Within 2%
    }
    
    #[test]
    fn test_fnv_hasher() {
        let mut hasher1 = FnvHasher::new_with_seed(0);
        let mut hasher2 = FnvHasher::new_with_seed(0);
        
        b"test".hash(&mut hasher1);
        b"test".hash(&mut hasher2);
        
        assert_eq!(hasher1.finish(), hasher2.finish());
        
        // Different seeds should produce different hashes
        let mut hasher3 = FnvHasher::new_with_seed(1);
        b"test".hash(&mut hasher3);
        
        assert_ne!(hasher1.finish(), hasher3.finish());
    }
}
