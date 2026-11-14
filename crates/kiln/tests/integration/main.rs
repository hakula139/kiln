#![expect(clippy::unwrap_used, reason = "test failures should panic")]

mod assets;
mod cli;
mod convert;
mod errors;
mod feeds;
#[path = "../support/fixtures.rs"]
mod fixtures;
#[path = "../support/git.rs"]
mod git;
mod listings;
mod options;
mod pages;
mod routes;
mod stylesheets;
mod support;
mod theme;
