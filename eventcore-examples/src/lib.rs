//! Example implementations using `EventCore` event sourcing library
//!
//! This crate provides example implementations demonstrating how to use
//! the `EventCore` library for various event sourcing scenarios.

#![forbid(unsafe_code)]
#![warn(missing_docs)]
// Allow nutype macros to work with current MSRV
#![allow(clippy::incompatible_msrv)]
// These are examples, so we don't need to be as pedantic
#![allow(clippy::missing_const_for_fn)]
#![allow(clippy::use_self)]

/// Banking example: Account management and money transfers with type-safe domain modeling
pub mod banking;

/// E-commerce example: Order workflow with inventory management
pub mod ecommerce;

/// Sagas example: Long-running distributed transactions
pub mod sagas;

/// Benchmarks: Performance testing examples
pub mod benchmarks;

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        // Placeholder test
        assert_eq!(2 + 2, 4);
    }
}
