extern crate core;

use std::collections::HashMap;
use std::env;
use std::str::FromStr;
use std::sync::mpsc::Sender;

use tracer::TraceMode;

use crate::fsm::{Event, EventType};

pub mod reader;

pub mod fsm_executor;
pub mod fsm;
pub mod executable_content;

#[cfg(feature = "ECMAScript")]
pub mod ecma_script_datamodel;

#[cfg(feature = "BasicHttpEventIOProcessor")]
pub mod basic_http_event_io_processor;

pub mod scxml_event_io_processor;

mod datamodel;
mod event_io_processor;
pub mod tracer;

pub fn handle_trace(sender: &mut Sender<Box<Event>>, opt: &str, enable: bool) {
    match TraceMode::from_str(opt) {
        Ok(t) => {
            let event = Box::new(Event::trace(t, enable));
            match sender.send(event) {
                Ok(_r) => {
                    // ok
                }
                Err(e) => {
                    eprintln!("Error sending trace event: {}", e);
                }
            }
        }
        Err(_e) => {
            println!("Unknown trace option. Use one of:\n methods\n states\n events\n arguments\n results\n all\n");
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
            name: name,
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
pub fn get_arguments(arguments: &[ArgOption]) -> (HashMap::<&'static str, String>, Vec<String>) {
    let mut final_args = Vec::<String>::new();

    let args: Vec<String> = env::args().collect();
    let mut idx = 1;
    let mut map = HashMap::new();

    // Don't use clap to parse arguments for now to reduce dependencies.
    while idx < args.len() {
        let arg = &args[idx];
        idx += 1;

        if arg.starts_with("-") {
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
