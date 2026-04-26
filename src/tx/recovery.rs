#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct RecoveryReport {
    pub recovered: usize,
    pub in_doubt: usize,
    pub pending: usize,
    pub tx_ids: Vec<String>,
}
