use crate::model::ScheduleDocument;
use crate::quantity::parse_quantity;

const MIN_MAX_BLOCK_GAS_LIMIT: u64 = 5000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ValidationFailure {
    EmptySchedule,
    FirstActivationBlockNotZero {
        actual: u64,
    },
    NonIncreasingActivationBlocks {
        index: usize,
        previous: u64,
        current: u64,
    },
    ElasticityMultiplierNotPositive {
        index: usize,
    },
    BaseFeeMaxChangeDenominatorNotPositive {
        index: usize,
    },
    InvalidQuantity {
        index: usize,
        field: &'static str,
        value: String,
    },
    MaxBlockGasLimitBelowMinimum {
        index: usize,
        value: u64,
    },
    ChainIdMismatch {
        expected: u64,
        actual: u64,
    },
    VersionRegression {
        offered: u64,
        last: u64,
    },
    VersionNotIncreased {
        version: u64,
    },
}

impl std::fmt::Display for ValidationFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptySchedule => write!(f, "schedule must not be empty"),
            Self::FirstActivationBlockNotZero { actual } => {
                write!(f, "schedule[0].activationBlock must be 0, got {actual}")
            }
            Self::NonIncreasingActivationBlocks {
                index,
                previous,
                current,
            } => write!(
                f,
                "schedule[{index}].activationBlock ({current}) must be strictly greater than the previous entry ({previous})"
            ),
            Self::ElasticityMultiplierNotPositive { index } => write!(
                f,
                "schedule[{index}].elasticityMultiplier must be greater than 0"
            ),
            Self::BaseFeeMaxChangeDenominatorNotPositive { index } => write!(
                f,
                "schedule[{index}].baseFeeMaxChangeDenominator must be greater than 0"
            ),
            Self::InvalidQuantity {
                index,
                field,
                value,
            } => write!(
                f,
                "schedule[{index}].{field} is not a valid decimal or 0x-hex quantity: {value}"
            ),
            Self::MaxBlockGasLimitBelowMinimum { index, value } => write!(
                f,
                "schedule[{index}].maxBlockGasLimit ({value}) must be at least {MIN_MAX_BLOCK_GAS_LIMIT}"
            ),
            Self::ChainIdMismatch { expected, actual } => {
                write!(f, "chainId {actual} does not match expected {expected}")
            }
            Self::VersionRegression { offered, last } => write!(
                f,
                "version {offered} is lower than the last published version {last}"
            ),
            Self::VersionNotIncreased { version } => write!(
                f,
                "content changed but version ({version}) was not increased"
            ),
        }
    }
}

pub fn validate_document(
    doc: &ScheduleDocument,
    expected_chain_id: Option<u64>,
) -> Result<(), ValidationFailure> {
    if doc.schedule.is_empty() {
        return Err(ValidationFailure::EmptySchedule);
    }

    if doc.schedule[0].activation_block != 0 {
        return Err(ValidationFailure::FirstActivationBlockNotZero {
            actual: doc.schedule[0].activation_block,
        });
    }

    for (window_index, window) in doc.schedule.windows(2).enumerate() {
        let previous = window[0].activation_block;
        let current = window[1].activation_block;
        if current <= previous {
            return Err(ValidationFailure::NonIncreasingActivationBlocks {
                index: window_index + 1,
                previous,
                current,
            });
        }
    }

    for (index, entry) in doc.schedule.iter().enumerate() {
        if entry.elasticity_multiplier == 0 {
            return Err(ValidationFailure::ElasticityMultiplierNotPositive { index });
        }
        if entry.base_fee_max_change_denominator == 0 {
            return Err(ValidationFailure::BaseFeeMaxChangeDenominatorNotPositive { index });
        }

        let min_base_fee = parse_quantity(&entry.min_base_fee_per_gas).ok_or(
            ValidationFailure::InvalidQuantity {
                index,
                field: "minBaseFeePerGas",
                value: entry.min_base_fee_per_gas.clone(),
            },
        )?;

        let max_block_gas_limit = parse_quantity(&entry.max_block_gas_limit).ok_or(
            ValidationFailure::InvalidQuantity {
                index,
                field: "maxBlockGasLimit",
                value: entry.max_block_gas_limit.clone(),
            },
        )?;

        if max_block_gas_limit < MIN_MAX_BLOCK_GAS_LIMIT {
            return Err(ValidationFailure::MaxBlockGasLimitBelowMinimum {
                index,
                value: max_block_gas_limit,
            });
        }

        let _ = min_base_fee;
    }

    if let Some(expected) = expected_chain_id
        && doc.chain_id != expected
    {
        return Err(ValidationFailure::ChainIdMismatch {
            expected,
            actual: doc.chain_id,
        });
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ScheduleDocument, ScheduleEntry};

    fn entry(activation_block: u64) -> ScheduleEntry {
        ScheduleEntry {
            activation_block,
            min_base_fee_per_gas: "440000000".to_string(),
            elasticity_multiplier: 2,
            base_fee_max_change_denominator: 8,
            max_block_gas_limit: "30000000".to_string(),
        }
    }

    fn document(schedule: Vec<ScheduleEntry>) -> ScheduleDocument {
        ScheduleDocument {
            chain_id: 42069,
            version: 1,
            current_block: None,
            schedule,
        }
    }

    #[test]
    fn valid_document_passes() {
        assert!(validate_document(&document(vec![entry(0)]), None).is_ok());
        assert!(
            validate_document(
                &document(vec![entry(0), entry(1_000), entry(1_000_000)]),
                None
            )
            .is_ok()
        );
    }

    #[test]
    fn rejects_empty_schedule() {
        assert_eq!(
            validate_document(&document(vec![]), None),
            Err(ValidationFailure::EmptySchedule)
        );
    }

    #[test]
    fn rejects_first_activation_block_not_zero() {
        let mut entry = entry(7);
        entry.activation_block = 7;
        assert_eq!(
            validate_document(&document(vec![entry]), None),
            Err(ValidationFailure::FirstActivationBlockNotZero { actual: 7 })
        );
    }

    #[test]
    fn rejects_non_increasing_activation_blocks() {
        let result = validate_document(&document(vec![entry(0), entry(0)]), None);
        assert_eq!(
            result,
            Err(ValidationFailure::NonIncreasingActivationBlocks {
                index: 1,
                previous: 0,
                current: 0
            })
        );

        let result = validate_document(&document(vec![entry(0), entry(10), entry(5)]), None);
        assert_eq!(
            result,
            Err(ValidationFailure::NonIncreasingActivationBlocks {
                index: 2,
                previous: 10,
                current: 5
            })
        );
    }

    #[test]
    fn rejects_non_positive_elasticity_multiplier() {
        let mut entry = entry(0);
        entry.elasticity_multiplier = 0;
        assert_eq!(
            validate_document(&document(vec![entry]), None),
            Err(ValidationFailure::ElasticityMultiplierNotPositive { index: 0 })
        );
    }

    #[test]
    fn rejects_non_positive_base_fee_max_change_denominator() {
        let mut entry = entry(0);
        entry.base_fee_max_change_denominator = 0;
        assert_eq!(
            validate_document(&document(vec![entry]), None),
            Err(ValidationFailure::BaseFeeMaxChangeDenominatorNotPositive { index: 0 })
        );
    }

    #[test]
    fn rejects_invalid_quantity_strings() {
        let mut min_fee_entry = entry(0);
        min_fee_entry.min_base_fee_per_gas = "nope".to_string();
        assert_eq!(
            validate_document(&document(vec![min_fee_entry]), None),
            Err(ValidationFailure::InvalidQuantity {
                index: 0,
                field: "minBaseFeePerGas",
                value: "nope".to_string()
            })
        );

        let mut gas_limit_entry = entry(0);
        gas_limit_entry.max_block_gas_limit = "0xzz".to_string();
        assert_eq!(
            validate_document(&document(vec![gas_limit_entry]), None),
            Err(ValidationFailure::InvalidQuantity {
                index: 0,
                field: "maxBlockGasLimit",
                value: "0xzz".to_string()
            })
        );
    }

    #[test]
    fn accepts_hex_quantities() {
        let mut entry = entry(0);
        entry.min_base_fee_per_gas = "0x1a3b".to_string();
        entry.max_block_gas_limit = "0x1e8480".to_string();
        assert!(validate_document(&document(vec![entry]), None).is_ok());
    }

    #[test]
    fn rejects_max_block_gas_limit_below_minimum() {
        let mut entry = entry(0);
        entry.max_block_gas_limit = "4999".to_string();
        assert_eq!(
            validate_document(&document(vec![entry]), None),
            Err(ValidationFailure::MaxBlockGasLimitBelowMinimum {
                index: 0,
                value: 4999
            })
        );
    }

    #[test]
    fn rejects_chain_id_mismatch() {
        assert_eq!(
            validate_document(&document(vec![entry(0)]), Some(1)),
            Err(ValidationFailure::ChainIdMismatch {
                expected: 1,
                actual: 42069
            })
        );
    }

    #[test]
    fn chain_id_check_skipped_when_expected_absent() {
        assert!(validate_document(&document(vec![entry(0)]), None).is_ok());
    }
}
