pub(crate) mod kmkc;
pub(crate) mod models;
pub(crate) mod musq;
pub(super) mod parser;
pub(crate) mod tools;

/// All available implementations
pub enum Implementations {
    /// KM by KC
    Kmkc,
    /// MU! by SQ
    Musq,
}