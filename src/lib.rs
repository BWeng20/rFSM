extern crate core;

use std::env;
use std::str::FromStr;
use std::sync::mpsc::Sender;

use crate::fsm::{Event, EventType, Trace};

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

pub fn handle_trace(sender: &mut Sender<Box<Event>>, opt: &str, enable: bool) {
    match Trace::from_str(opt) {
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

pub fn get_arguments() -> (Trace, Vec::<String>) {
    let mut final_args = Vec::<String>::new();

    let args: Vec<String> = env::args().collect();
    let mut idx = 1;
    // Default for trace option.
    let mut trace = Trace::STATES;

    // Don't use clap to parse arguments for now to reduce dependencies.
    while idx < args.len() {
        let arg = &args[idx];
        idx += 1;

        if arg.starts_with("-") {
            let sarg = arg.trim_start_matches('-');
            match sarg {
                "trace" => {
                    if idx >= args.len() {
                        panic!("Missing arguments");
                    }
                    let trace_opt = &args[idx];
                    idx += 1;
                    match Trace::from_str(trace_opt) {
                        Ok(t) => {
                            trace = t;
                        }
                        Err(_e) => {
                            panic!("Unsupported trace option {}.", trace_opt);
                        }
                    }
                }
                _ => {
                    panic!("Unsupported option {}", sarg);
                }
            }
        } else {
            final_args.push(arg.clone());
        }
    }
    (trace, final_args)
}
