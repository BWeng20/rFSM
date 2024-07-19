use std::fs::File;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};
use std::process;
use std::sync::mpsc::Sender;
#[cfg(test)]
use std::{println as error, println as info};

#[cfg(not(test))]
use log::{error, info};
#[cfg(feature = "json-config")]
use serde::Deserialize;
#[cfg(feature = "yaml-config")]
use yaml_rust::YamlLoader;

use crate::fsm::{Event, Fsm};
use crate::fsm_executor::FsmExecutor;
#[cfg(feature = "Trace")]
use crate::test_tracer::{abort_test, TestTracer};
#[cfg(feature = "Trace")]
use crate::tracer::{TraceMode, Tracer};
use crate::{fsm, scxml_reader};

#[cfg_attr(feature = "json-config", derive(Deserialize))]
pub struct EventSpecification {
    /// Mandatory event name to send.
    name: String,

    /// Delay in milliseconds after the event was sent.
    delay_ms: i32,

    /// Optional state to reach after the event\
    /// Use "#stop" to check for termination of FSM.
    shall_reach_state: Option<String>,

    /// Optional event to receive from FSM after the event.
    shall_send_event: Option<String>,
}

#[cfg_attr(feature = "json-config", derive(Deserialize))]
pub struct TestSpecification {
    pub file: Option<String>,
    events: Vec<EventSpecification>,
    final_configuration: Option<Vec<String>>,
    timeout_milliseconds: Option<i32>,
}

pub struct TestUseCase {
    pub name: String,
    pub specification: TestSpecification,
    pub fsm: Option<Box<Fsm>>,
    pub trace_mode: TraceMode,
    pub include_paths: Vec<PathBuf>,
}

pub fn load_fsm(file_path: &str, include_paths: &Vec<PathBuf>) -> Result<Box<Fsm>, String> {
    scxml_reader::parse_from_xml_file(Path::new(file_path), include_paths)
}

#[cfg(feature = "yaml-config")]
pub fn load_yaml_config(file_path: &str) -> TestSpecification {
    match File::open(file_path) {
        Ok(file) => {
            let mut reader = BufReader::new(file);

            let mut yaml = String::new();
            match reader.read_to_string(&mut yaml) {
                Ok(_) => match YamlLoader::load_from_str(&yaml) {
                    Ok(_doc) => {
                        todo!()
                    }
                    Err(err) => {
                        abort_test(format!(
                            "Error de-serializing config file '{}'. {}",
                            file_path, err
                        ));
                    }
                },
                Err(err) => {
                    abort_test(format!(
                        "Error reading config file '{}'. {}",
                        file_path, err
                    ));
                }
            }
        }
        Err(err) => {
            abort_test(format!(
                "Error reading config file '{}'. {}",
                file_path, err
            ));
        }
    }
}

#[cfg(feature = "json-config")]
pub fn load_json_config(file_path: &str) -> TestSpecification {
    match File::open(file_path) {
        Ok(file) => {
            let reader = BufReader::new(file);
            match serde_json::from_reader::<BufReader<File>, TestSpecification>(reader) {
                Ok(test) => test,
                Err(err) => {
                    abort_test(format!(
                        "Error de-serializing config file '{}'. {}",
                        file_path, err
                    ));
                }
            }
        }
        Err(err) => {
            abort_test(format!(
                "Error reading config file '{}'. {}",
                file_path, err
            ));
        }
    }
}

pub fn run_test(test: TestUseCase) {
    if test.fsm.is_none() {
        abort_test(format!("No FSM given in test '{}'", test.name))
    }

    let fsm = test.fsm.unwrap();

    let timeout = test.specification.timeout_milliseconds.unwrap_or(0);
    let final_expected_configuration = test.specification.final_configuration.unwrap_or(Vec::new());

    run_test_manual(
        &test.name,
        fsm,
        &test.include_paths,
        test.trace_mode,
        timeout as u64,
        &final_expected_configuration,
    );
    process::exit(0);
}

pub fn run_test_manual(
    test_name: &str,
    fsm: Box<Fsm>,
    include_paths: &Vec<PathBuf>,
    trace_mode: TraceMode,
    timeout: u64,
    expected_final_configuration: &Vec<String>,
) -> bool {
    run_test_manual_with_send(
        test_name,
        fsm,
        include_paths,
        trace_mode,
        timeout,
        expected_final_configuration,
        move |_sender| {},
    )
}

pub fn run_test_manual_with_send(
    test_name: &str,
    mut fsm: Box<Fsm>,
    include_paths: &Vec<PathBuf>,
    trace_mode: TraceMode,
    timeout: u64,
    expected_final_configuration: &Vec<String>,
    mut cb: impl FnMut(Sender<Box<Event>>),
) -> bool {
    let mut tracer = Box::new(TestTracer::new());
    tracer.enable_trace(trace_mode);
    let current_config = tracer.get_fsm_config();
    fsm.tracer = tracer;

    let mut executer = FsmExecutor::new_without_io_processor();
    for ip in include_paths {
        executer.include_paths.push(ip.clone());
    }
    let session = fsm::start_fsm(fsm, Box::new(executer));

    let mut watchdog_sender: Option<Box<Sender<String>>> = None;
    if timeout > 0 {
        watchdog_sender = Some(TestTracer::start_watchdog(test_name, timeout));
    }

    // Sending some event
    cb(session.sender);

    info!("FSM started. Waiting to terminate...");
    let _ = session.session_thread.unwrap().join();

    if watchdog_sender.is_some() {
        // Inform watchdog
        TestTracer::disable_watchdog(&watchdog_sender.unwrap());
    }

    if expected_final_configuration.is_empty() {
        true
    } else {
        match TestTracer::verify_final_configuration(&expected_final_configuration, &current_config)
        {
            Ok(states) => {
                info!(
                    "[{}] ==> Final configuration '{}' reached",
                    test_name, states
                );
                true
            }
            Err(states) => {
                let mut config_states = Vec::new();
                let guard = current_config.lock();
                if guard.is_ok() {
                    for name in guard.unwrap().keys() {
                        config_states.push(name.clone());
                    }
                }
                error!(
                    "[{}] ==> Expected final state '{}' not reached. Final configuration: {}",
                    test_name,
                    states,
                    config_states.join(",")
                );
                false
            }
        }
    }
}
