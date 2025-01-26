//! A Finite State Machine (FSM) Implementation in Rust.\
//! \
//! According to W3C Recommendations, reading State Chart XML (SCXML).\
//! See /doc/W3C_SCXML_2024_07_13/index.html
//!

extern crate core;

pub mod executable_content;
pub mod fsm;
pub mod fsm_executor;
#[cfg(feature = "xml")]
pub mod scxml_reader;

#[cfg(feature = "serializer")]
pub mod serializer;

#[cfg(feature = "Trace")]
pub mod tracer;

#[cfg(feature = "TraceServer")]
pub mod remote_tracer;

pub mod actions;
pub mod common;
pub mod datamodel;
pub mod event_io_processor;
pub mod expression_engine;
pub mod test;
