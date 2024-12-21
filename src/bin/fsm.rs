//! Demonstration and Test application.
//! Usage:
//!    rfsm \<scxml-file\> \[-trace flag\]
extern crate core;

use log::error;
#[cfg(feature = "ECMAScript")]
use rfsm::ecma_script_datamodel::ECMA_STRICT_ARGUMENT;
use std::io::{stdout, Write};
use std::{io, process, thread, time};

use rfsm::actions::ActionWrapper;
use rfsm::fsm::{Event, EventType};
use rfsm::fsm_executor::FsmExecutor;
#[cfg(feature = "Trace")]
use rfsm::handle_trace;
use rfsm::init_logging;
#[cfg(feature = "xml")]
use rfsm::scxml_reader::INCLUDE_PATH_ARGUMENT_OPTION;
#[cfg(feature = "Trace")]
use rfsm::tracer::{TraceMode, TRACE_ARGUMENT_OPTION};

/// Loads the specified FSM and prompts for Events.
#[tokio::main(flavor = "multi_thread")]
async fn main() {
    init_logging();

    let (named_opt, final_args) = rfsm::get_arguments(&[
        #[cfg(feature = "Trace")]
        &TRACE_ARGUMENT_OPTION,
        #[cfg(feature = "xml")]
        &INCLUDE_PATH_ARGUMENT_OPTION,
        #[cfg(feature = "ECMAScript")]
        &ECMA_STRICT_ARGUMENT,
    ]);

    #[cfg(feature = "Trace")]
    let trace = TraceMode::from_arguments(&named_opt);

    if final_args.is_empty() {
        println!("Missing argument. Please specify one or more scxml file");
        process::exit(1);
    }

    let mut executor = FsmExecutor::new_with_io_processor().await;
    #[cfg(feature = "xml")]
    executor.set_include_paths_from_arguments(&named_opt);
    executor.set_global_options_from_arguments(&named_opt);

    #[allow(unused_mut)]
    let mut session;

    match executor.execute(
        final_args[0].as_str(),
        ActionWrapper::new(),
        #[cfg(feature = "Trace")]
        trace,
    ) {
        Ok(s) => {
            session = s;
        }
        Err(err) => {
            error!("Failed to execute {}: {}", final_args[0], err);
            process::exit(1);
        }
    };

    for fi in &final_args[1..final_args.len()] {
        let _ = executor
            .execute(
                fi.as_str(),
                ActionWrapper::new(),
                #[cfg(feature = "Trace")]
                trace,
            )
            .unwrap();
    }

    let mut line = String::new();
    let stdin = io::stdin();

    loop {
        // let the FSM some time to process.
        // only needed to ensure that the prompt will be printed after normal FSM output.
        thread::sleep(time::Duration::from_millis(200));

        // If FSM was reached final state(s) the worker thread will be finished.
        match &session.thread {
            None => {}
            Some(thread) => {
                if thread.is_finished() {
                    println!("\nSM finished!");
                    executor.shutdown();
                    // TODO: dump data from the "finish"
                    break;
                }
            }
        }
        print!("\nEnter Event >>");
        let _ = stdout().flush();
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
                    #[cfg(feature = "Trace")]
                    handle_trace(&mut session.sender, &line_lc[5..], true);
                } else if line_lc.starts_with("troff") && line_lc.len() > 6 {
                    #[cfg(feature = "Trace")]
                    handle_trace(&mut session.sender, &line_lc[6..], false);
                } else if !line_lc.eq("help") && !line.is_empty() {
                    let event = Box::new(Event {
                        name: line.clone(),
                        etype: EventType::platform,
                        sendid: None,
                        origin: None,
                        origin_type: None,
                        invoke_id: None,
                        param_values: None,
                        content: None,
                    });
                    match session.sender.send(event) {
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
                    println!(
                        r#"Usage:
Use 'Tron <flag>' or 'Troff <flag>' to control trace-levels.
E.g. enter: tron all
To send events, type the name of the event and press enter.
Remind that Events are case sensitive.
To print this information enter 'help' or an empty line.
"#
                    );
                }
            }

            Err(e) => {
                eprintln!("Error: {}. aborting...", e);
                process::exit(-1);
            }
        }
    }
}
