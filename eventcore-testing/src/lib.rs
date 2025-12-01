#![deny(warnings)]
#![forbid(
    dead_code,
    invalid_value,
    overflowing_literals,
    unconditional_recursion,
    unreachable_pub,
    unused_allocation,
    unsafe_code
)]
#![deny(
    bad_style,
    clippy::allow_attributes,
    deprecated,
    meta_variable_misuse,
    non_ascii_idents,
    non_camel_case_types,
    non_snake_case,
    non_upper_case_globals,
    rust_2018_idioms,
    rust_2021_compatibility,
    trivial_casts,
    trivial_numeric_casts,
    unreachable_code,
    unused_assignments,
    unused_attributes,
    unused_extern_crates,
    unused_imports,
    unused_must_use,
    unused_mut,
    unused_parens,
    unused_qualifications,
    unused_results,
    unused_variables
)]

pub mod chaos;

pub use chaos::*;
