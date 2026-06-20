use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use time::macros::format_description;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ScheduleDocument {
    #[serde(rename = "chainId")]
    pub chain_id: u64,
    pub version: u64,
    #[serde(
        rename = "currentBlock",
        skip_serializing_if = "Option::is_none",
        default
    )]
    pub current_block: Option<u64>,
    pub schedule: Vec<ScheduleEntry>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ScheduleEntry {
    #[serde(rename = "activationBlock")]
    pub activation_block: u64,
    #[serde(rename = "minBaseFeePerGas")]
    pub min_base_fee_per_gas: String,
    #[serde(rename = "elasticityMultiplier")]
    pub elasticity_multiplier: u64,
    #[serde(rename = "baseFeeMaxChangeDenominator")]
    pub base_fee_max_change_denominator: u64,
    #[serde(rename = "maxBlockGasLimit")]
    pub max_block_gas_limit: String,
}

pub fn canonicalize(doc: &ScheduleDocument) -> String {
    serde_json::to_string_pretty(doc).expect("schedule document serializes to JSON")
}

pub fn select_active_entries(doc: &ScheduleDocument) -> &[ScheduleEntry] {
    if doc.schedule.is_empty() {
        return &doc.schedule;
    }

    let Some(current) = doc.current_block else {
        return &doc.schedule;
    };

    let mut count = doc
        .schedule
        .iter()
        .take_while(|entry| entry.activation_block <= current)
        .count();

    if count == 0 {
        count = 1;
    }

    &doc.schedule[..count]
}

pub fn now_iso_second() -> String {
    let format = format_description!("[year]-[month]-[day]T[hour]:[minute]:[second]Z");
    OffsetDateTime::now_utc()
        .format(format)
        .expect("format infallible for fixed description")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(activation_block: u64) -> ScheduleEntry {
        ScheduleEntry {
            activation_block,
            min_base_fee_per_gas: "440000000".to_string(),
            elasticity_multiplier: 2,
            base_fee_max_change_denominator: 8,
            max_block_gas_limit: "30000000".to_string(),
        }
    }

    fn doc(current_block: Option<u64>, schedule: Vec<ScheduleEntry>) -> ScheduleDocument {
        ScheduleDocument {
            chain_id: 42069,
            version: 1,
            current_block,
            schedule,
        }
    }

    #[test]
    fn canonicalize_omits_absent_current_block() {
        let serialized = canonicalize(&doc(None, vec![entry(0)]));
        assert!(serialized.contains("\"chainId\""));
        assert!(!serialized.contains("currentBlock"));
        assert!(serialized.contains("\"activationBlock\""));
    }

    #[test]
    fn select_returns_full_schedule_when_current_block_absent() {
        let document = doc(None, vec![entry(0), entry(100), entry(200)]);
        assert_eq!(select_active_entries(&document).len(), 3);
    }

    #[test]
    fn select_filters_entries_above_current_block() {
        let document = doc(Some(150), vec![entry(0), entry(100), entry(200)]);
        let active = select_active_entries(&document);
        assert_eq!(active.len(), 2);
        assert_eq!(active[0].activation_block, 0);
        assert_eq!(active[1].activation_block, 100);
    }

    #[test]
    fn select_falls_back_to_first_entry() {
        let document = doc(Some(0), vec![entry(0), entry(100)]);
        let active = select_active_entries(&document);
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].activation_block, 0);
    }
}
