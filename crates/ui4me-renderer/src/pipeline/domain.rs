//! Runtime policies owned by the update pipeline. Diagnostic exports observe
//! these values; they do not define rendering semantics.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ReadMode {
    Flush,
    Committed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClockMode {
    Realtime,
    Manual,
}
