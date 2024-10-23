//! A Finite State Machine (FSM) Implementation in Rust.\
//! \
//! According to W3C Recommendations, reading State Chart XML (SCXML).\
//! See /doc/W3C_SCXML_2024_07_13/index.html
//!

extern crate core;

#[cfg(feature = "EnvLog")]
use chrono::Local;
use std::collections::HashMap;
use std::env;
#[cfg(feature = "EnvLog")]
use std::io::Write;
#[cfg(feature = "Trace")]
use std::str::FromStr;
#[cfg(feature = "Trace")]
use std::sync::mpsc::Sender;

#[cfg(feature = "Trace")]
use log::error;

#[cfg(feature = "Trace")]
use tracer::TraceMode;

use crate::fsm::{Event, EventType};

#[cfg(feature = "xml")]
pub mod scxml_reader;

pub mod executable_content;
pub mod fsm;
pub mod fsm_executor;

#[cfg(feature = "ECMAScript")]
pub mod ecma_script_datamodel;

#[cfg(feature = "BasicHttpEventIOProcessor")]
pub mod basic_http_event_io_processor;

pub mod scxml_event_io_processor;

#[cfg(feature = "serializer")]
pub mod serializer;

pub mod datamodel;
pub mod event_io_processor;

#[cfg(feature = "Trace")]
pub mod tracer;

#[cfg(feature = "TraceServer")]
pub mod remote_tracer;

pub mod actions;
pub mod expression_parser;
pub mod expressions;
pub mod test;

#[cfg(feature = "Trace")]
pub fn handle_trace(sender: &mut Sender<Box<Event>>, opt: &str, enable: bool) {
    match TraceMode::from_str(opt) {
        Ok(t) => {
            let event = Box::new(Event::trace(t, enable));
            match sender.send(event) {
                Ok(_r) => {
                    // ok
                }
                Err(e) => {
                    error!("Error sending trace event: {}", e);
                }
            }
        }
        Err(_e) => {
            eprintln!("Unknown trace option. Use one of:\n methods\n states\n events\n arguments\n results\n all\n");
        }
    }
}

/// Descriptor a program argument option
pub struct ArgOption {
    pub name: &'static str,
    pub required: bool,
    pub with_value: bool,
}

impl ArgOption {
    /// Creates a new option with the specified name.
    pub fn new(name: &'static str) -> ArgOption {
        ArgOption {
            name,
            required: false,
            with_value: false,
        }
    }

    /// Defines this option as "required".
    pub fn required(mut self) -> Self {
        self.required = true;
        self
    }

    /// Defines that this option needs a value.
    pub fn with_value(mut self) -> Self {
        self.with_value = true;
        self
    }
}

/// Parse program arguments.
pub fn get_arguments(arguments: &[&ArgOption]) -> (HashMap<&'static str, String>, Vec<String>) {
    let mut final_args = Vec::<String>::new();

    let args: Vec<String> = env::args().collect();
    let mut idx = 1;
    let mut map = HashMap::new();

    // Don't use clap to parse arguments for now to reduce dependencies.
    while idx < args.len() {
        let arg = &args[idx];
        idx += 1;

        if arg.starts_with('-') {
            let sarg = arg.trim_start_matches('-');
            let mut match_found = false;
            for opt in arguments {
                match_found = opt.name == sarg;
                if match_found {
                    if opt.with_value {
                        if idx >= args.len() {
                            panic!("Missing value for argument '{}'", opt.name);
                        }
                        map.insert(opt.name, args[idx].clone());
                        idx += 1;
                    } else {
                        map.insert(opt.name, "".to_string());
                    }
                    break;
                }
            }
            if !match_found {
                panic!("Unknown option '{}'", arg);
            }
        } else {
            final_args.push(arg.clone());
        }
    }
    (map, final_args)
}

pub fn init_logging() {
    #[cfg(feature = "EnvLog")]
    {
        env_logger::builder()
            .format(|buf, record| {
                let thread_name = {
                    if let Some(n) = std::thread::current().name() {
                        n.to_string()
                    } else {
                        format!("{:?}", std::thread::current().id())
                    }
                };
                writeln!(
                    buf,
                    "{} [{:8}] {:5} {}",
                    Local::now().format("%m-%d %H:%M:%S%.3f"),
                    thread_name,
                    record.level(),
                    record.args()
                )
            })
            .init();
    }
}
