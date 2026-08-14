//! Rust-native behavior-test and documentation verification tooling.

mod documentation;
mod runner;

use std::ffi::OsString;

pub(crate) fn run_tests(args: &[OsString]) -> i32 {
    runner::run(args)
}

pub(crate) fn run_isolation_proof(args: &[OsString]) -> i32 {
    runner::run_isolation_proof(args)
}

pub(crate) fn run_documentation_check(args: &[OsString]) -> i32 {
    documentation::run(args)
}
