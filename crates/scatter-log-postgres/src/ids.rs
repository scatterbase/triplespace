//! 64-bit IDs in `bigint` columns: partition IDs are 64 random bits (0018 §2), so they
//! are stored by reinterpretation, not by value. Offsets, sizes and times never reach
//! the sign bit, but go through the same conversions so a stray value is an error,
//! not a wrap.

use scatter_log::store::StoreError;

/// A partition ID as the `bigint` that stores it.
#[must_use]
#[allow(clippy::cast_possible_wrap)]
pub fn partition_to_db(partition: u64) -> i64 {
    partition as i64
}

/// A partition ID from its `bigint`.
#[must_use]
#[allow(clippy::cast_sign_loss)]
pub fn partition_from_db(db: i64) -> u64 {
    db as u64
}

/// A non-negative quantity (an offset, a size, a time) as `bigint`.
pub fn to_db(n: u64) -> Result<i64, StoreError> {
    i64::try_from(n).map_err(|_| StoreError::Corrupt(format!("{n} does not fit a bigint")))
}

/// A non-negative quantity from `bigint`.
pub fn from_db(db: i64) -> Result<u64, StoreError> {
    u64::try_from(db).map_err(|_| StoreError::Corrupt(format!("a negative value {db} in the log")))
}

/// An optional quantity from a nullable `bigint`.
pub fn opt_from_db(db: Option<i64>) -> Result<Option<u64>, StoreError> {
    db.map(from_db).transpose()
}

/// The child table of a partition's records.
#[must_use]
pub fn record_table(partition: u64) -> String {
    format!("log.record_{partition:016x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partitions_round_trip_through_the_sign_bit() {
        for p in [0, 1, u64::MAX, 1 << 63, 12_345_678_901_234_567_890] {
            assert_eq!(partition_from_db(partition_to_db(p)), p);
        }
        assert!(partition_to_db(u64::MAX) < 0);
        assert!(to_db(u64::MAX).is_err());
        assert!(from_db(-1).is_err());
        assert_eq!(opt_from_db(None).unwrap(), None);
        assert_eq!(record_table(0x42), "log.record_0000000000000042");
    }
}
