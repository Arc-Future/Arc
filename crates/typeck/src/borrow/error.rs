use thiserror::Error;

#[derive(Debug, Error, PartialEq)]
pub enum BorrowError {
    #[error("use of moved value `{0}`")]
    UseAfterMove(String),
    #[error("cannot borrow `{0}` as mutable because it is already borrowed")]
    AlreadyBorrowed(String),
    #[error("cannot borrow `{0}` as immutable because it is mutably borrowed")]
    MutablyBorrowed(String),
}
