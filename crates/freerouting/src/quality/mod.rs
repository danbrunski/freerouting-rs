//! Routing-quality reports and stages beyond Java (the Q series, see
//! `docs/roadmap/routing-quality.md`).
//!
//! Each checklist appends its own `pub mod` line here.

pub mod attribution;
pub mod congestion;
pub mod feasibility;
pub mod neckdown;
pub mod pour_audit;
pub mod pour_guard;
pub mod register;
pub mod sweep;
pub mod verdict;
