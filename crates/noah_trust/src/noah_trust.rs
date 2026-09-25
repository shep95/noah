//! noah's trust layer: the checks that sit between shepherd and the person's
//! code, so that what shepherd claims can be verified and what it touches can
//! be audited. Everything here is plain logic with no UI and no network, so
//! each piece can be tested on its own; the agent tools and mission control
//! room build on it.

pub mod blast_radius;
pub mod calibration;
pub mod cost;
pub mod evidence;
pub mod injection;
pub mod mutation;
pub mod outcomes;
pub mod packages;
pub mod planning;
pub mod project_files;
pub mod provenance;
pub mod rules_check;
pub mod secrets;
pub mod spec;
