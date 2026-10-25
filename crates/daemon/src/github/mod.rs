//! Daemon-owned GitHub App integration. Agents never receive GitHub credentials.
pub mod api;
pub mod auth;
pub mod commands;
pub mod gates;
pub mod setup;
pub mod sync;

pub mod git_ops;
pub mod lifecycle;

pub mod sprints;

pub mod milestones;

pub mod loop_sync;

pub mod status;
