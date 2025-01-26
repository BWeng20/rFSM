//! Demonstration and Test application.
//! Usage:
//!    rfsm \<scxml-file\> \[-trace flag\]
extern crate core;

use log::error;
#[cfg(feature = "ECMAScriptModel")]
use rufsm::datamodel::ecma_script::ECMA_STRICT_ARGUMENT;
use std::io::{stdout, Write};
use std::sync::mpsc::Sender;
use std::{io, process, thread, time};

use rufsm::actions::ActionWrapper;
#[cfg(feature = "Trace")]
use rufsm::common::handle_trace;
use rufsm::common::init_logging;
use rufsm::fsm::{Event, EventType};
use rufsm::fsm_executor::FsmExecutor;
#[cfg(feature = "xml")]
use rufsm::scxml_reader::INCLUDE_PATH_ARGUMENT_OPTION;
#[cfg(feature = "Trace")]
use rufsm::tracer::{TraceMode, TRACE_ARGUMENT_OPTION};

#[allow(unused_mut)]
fn input_loop(mut sender: Sender<Box<Event>>) {
    let mut line = String::new();
    let stdin = io::stdin();
    loop {
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
                    handle_trace(&mut sender, &line_lc[5..], true);
                } else if line_lc.starts_with("troff") && line_lc.len() > 6 {
                    #[cfg(feature = "Trace")]
                    handle_trace(&mut sender, &line_lc[6..], false);
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

#[cfg(not(feature = "rocket"))]
#[tokio::main(flavor = "multi_thread")]
async fn main() {
    main_internal().await;
}

// If using rocket, we need to initialize tokio the rocket way
#[cfg(feature = "rocket")]
#[rocket::main]
async fn main() {
    main_internal().await;
}

/// Loads the specified FSM and prompts for Events.
async fn main_internal() {
    init_logging();

    let (named_opt, final_args) = rufsm::common::get_arguments(&[
        #[cfg(feature = "Trace")]
        &TRACE_ARGUMENT_OPTION,
        #[cfg(feature = "xml")]
        &INCLUDE_PATH_ARGUMENT_OPTION,
        #[cfg(feature = "ECMAScriptModel")]
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

    if let Some(session_thread_join_handle) = session.thread {
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

        let sender_clone = session.sender.clone();

        // let the FSM some time to process.
        // only needed to ensure that the prompt will be printed after normal FSM output.
        thread::sleep(time::Duration::from_millis(200));

        match thread::Builder::new()
            .name("input".to_string())
            .spawn(move || input_loop(sender_clone))
        {
            Ok(_) => {
                let _ = session_thread_join_handle.join();
                println!("\nSM finished!");
                executor.shutdown();
                // TODO: dump data from the "finish"
                thread::sleep(time::Duration::from_millis(200));
                process::exit(0);
            }
            Err(error) => {
                error!("Failed to spawn input loop {}", error);
                process::exit(1);
            }
        }
    } else {
        error!("Failed to spawn FSM");
        process::exit(1);
    }
}
