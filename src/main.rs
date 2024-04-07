//! Demonstration and Test appliocation.
//! Usage:
//!    rfsm scxmlfile \[-trace flag\]
extern crate core;

use std::{env, io, process, thread, time};
use std::io::{stdout, Write};
use std::net::SocketAddr;
use std::str::FromStr;
use std::sync::mpsc::Sender;

use crate::basic_http_event_io_processor::BasicHTTPEventIOProcessor;
use crate::event_io_processor::EventIOProcessor;
use crate::fsm::{Event, EventType, Trace};

pub mod reader;
pub mod fsm;
pub mod executable_content;

#[cfg(feature = "ECMAScript")]
pub mod ecma_script_datamodel;

#[cfg(feature = "BasicHttpEventIOProcessor")]
pub mod basic_http_event_io_processor;

mod datamodel;
mod event_io_processor;

fn handle_trace(sender: &mut Sender<Box<Event>>, opt: &str, enable: bool) {
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

/// Loads the specified FSM and prompts for Events.
fn main() {
    env_logger::init();

    // Default for trace option.
    let mut trace = Trace::STATES;

    let mut final_args = Vec::<String>::new();

    let args: Vec<String> = env::args().collect();
    let mut idx = 1;
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

    if final_args.len() < 1 {
        println!("Missing argument. Please specify a scxml file");
        process::exit(1);
    }

    println!("Loading FSM from {}", final_args[0]);

    let mut processors: Vec<Box<dyn EventIOProcessor>> = Vec::new();

    #[cfg(feature = "BasicHttpEventIOProcessor")]
    processors.push(Box::new(BasicHTTPEventIOProcessor::new(&SocketAddr::from(([127, 0, 0, 1], 5555)))));

    // Use reader to parse the scxml file:
    match reader::read_from_xml_file(final_args[0].clone()) {
        Ok(mut sm) => {
            // Use reader to parse the scxml file:
            sm.tracer.enable_trace(trace);
            let (thread_handle, mut sender) = fsm::start_fsm(sm, &processors);

            let mut line = String::new();
            let stdin = io::stdin();
            let empty_str = "".to_string();

            loop {
                // let the FSM some time to process.
                // only needed to ensure that the prompt will be printed after normal FSM output.
                thread::sleep(time::Duration::from_millis(200));

                // If FSM was reached final state(s) the worker thread will be finished.
                if thread_handle.is_finished() {
                    println!("\nSM finished!");

                    for p in processors
                    {
                        p.shutdown();
                    }

                    // TODO: dump data from the "finish"

                    process::exit(0);
                } else {
                    print!("\nEnter Event >>");
                    match stdout().flush() {
                        _ => {}
                    }
                    line.clear();
                    match stdin.read_line(&mut line) {
                        Ok(_s) => {
                            if line.ends_with('\n') {
                                line.pop();
                                if line.ends_with('\r') {
                                    line.pop();
                                }
                            }
                            let line_lc = line.to_lowercase();
                            if line_lc.starts_with("tron") && line.len() > 5 {
                                handle_trace(&mut sender, &line_lc[5..], true);
                            } else if line_lc.starts_with("troff") && line_lc.len() > 6 {
                                handle_trace(&mut sender, &line_lc[6..], false);
                            } else if !line_lc.eq("help") && !line.is_empty() {
                                let event = Box::new(Event {
                                    name: line.clone(),
                                    etype: EventType::platform,
                                    sendid: empty_str.clone(),
                                    origin: empty_str.clone(),
                                    origintype: empty_str.clone(),
                                    invokeid: 1,
                                    data: None,
                                });
                                match sender.send(event) {
                                    Ok(_r) => {
                                        // ok
                                    }
                                    Err(e) => {
                                        eprintln!("Error sending event: {}", e);
                                        eprintln!("Aborting...");
                                        process::exit(-2);
                                    }
                                }
                            } else {
                                println!(r#"Usage:
  Use 'Tron <flag>' or 'Troff <flag>' to control trace-levels.
  E.g. enter: tron all
  To send events, type the name of the event and press enter.
  Remind that Events are case sensitive.
  To print this information enter 'help' or an empty line.
  "#);
                            }
                        }

                        Err(e) => {
                            eprintln!("Error: {}. aborting...", e);
                            process::exit(-1);
                        }
                    }
                }
            }
        }
        Err(e) => {
            eprintln!("Failed to open {} error {}", args[0], e);
            process::exit(2);
        }
    }
}


