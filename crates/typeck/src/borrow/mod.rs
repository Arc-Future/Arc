mod binding;
mod check_expr;
mod check_stmt;
mod checker;
mod error;

pub use checker::BorrowChecker;
pub use error::BorrowError;

#[cfg(test)]
mod tests;
