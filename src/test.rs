use std::collections::HashMap;
use std::fs::File;
use std::io::BufReader;
#[cfg(feature = "yaml-config")]
use std::io::Read;
use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::mpsc::Sender;
use std::time::Duration;
#[cfg(test)]
use std::{println as error, println as info};
use std::{process, thread};

use crate::actions::ActionWrapper;
use log::warn;
#[cfg(not(test))]
use log::{error, info};
#[cfg(feature = "json-config")]
use serde::Deserialize;
#[cfg(feature = "yaml-config")]
use yaml_rust::YamlLoader;

use crate::fsm;
use crate::fsm::{Event, FinishMode, Fsm};
use crate::fsm_executor::FsmExecutor;
#[cfg(feature = "xml")]
use crate::scxml_reader;
#[cfg(feature = "serializer")]
use crate::serializer::default_protocol_reader::DefaultProtocolReader;
#[cfg(feature = "serializer")]
use crate::serializer::fsm_reader::FsmReader;
#[cfg(feature = "Trace")]
use crate::tracer::TraceMode;

#[cfg_attr(feature = "json-config", derive(Deserialize))]
#[allow(unused)]
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
#[allow(unused)]
pub struct TestSpecification {
    pub file: Option<String>,
    events: Vec<EventSpecification>,
    final_configuration: Option<Vec<String>>,
    timeout_milliseconds: Option<i32>,
    options: Option<HashMap<String, String>>,
}

pub struct TestUseCase {
    pub name: String,
    pub specification: TestSpecification,
    pub fsm: Option<Box<Fsm>>,
    #[cfg(feature = "Trace")]
    pub trace_mode: TraceMode,
    pub include_paths: Vec<PathBuf>,
}

#[allow(unused_variables)]
pub fn load_fsm(file_path: &str, include_paths: &[PathBuf]) -> Result<Box<Fsm>, String> {
    let extension = file_path.rsplit('.').next().unwrap_or_default();

    #[cfg(feature = "xml")]
    if extension.eq_ignore_ascii_case("scxml") || extension.eq_ignore_ascii_case("xml") {
        return scxml_reader::parse_from_uri(file_path.to_string(), include_paths);
    }
    #[cfg(feature = "serializer")]
    if extension.eq_ignore_ascii_case("rfsm") {
        return match File::open(file_path) {
            Ok(f) => {
                let protocol = DefaultProtocolReader::new(BufReader::new(f));
                let mut reader = FsmReader::new(Box::new(protocol));
                reader.read()
            }
            Err(err) => Err(err.to_string()),
        };
    }
    Err(format!("No reader to load '{}'", file_path))
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
    let final_expected_configuration = test.specification.final_configuration.unwrap_or_default();

    let mut options_s = HashMap::new();

    if let Some(test_option) = test.specification.options {
        for (name, value) in &test_option {
            options_s.insert(name.clone(), value.clone());
        }
    }

    let mut options = HashMap::new();
    for (name, value) in &options_s {
        options.insert(name.as_str(), value.clone());
    }

    if !run_test_manual(
        &test.name,
        &options,
        fsm,
        &test.include_paths,
        #[cfg(feature = "Trace")]
        test.trace_mode,
        timeout as u64,
        &final_expected_configuration,
    ) {
        process::exit(-1);
    } else {
        process::exit(0);
    }
}

pub fn run_test_manual(
    test_name: &str,
    options: &HashMap<&str, String>,
    fsm: Box<Fsm>,
    include_paths: &Vec<PathBuf>,
    #[cfg(feature = "Trace")] trace_mode: TraceMode,
    timeout: u64,
    expected_final_configuration: &Vec<String>,
) -> bool {
    run_test_manual_with_send(
        test_name,
        options,
        fsm,
        include_paths,
        #[cfg(feature = "Trace")]
        trace_mode,
        timeout,
        expected_final_configuration,
        move |_sender| {},
    )
}

#[allow(clippy::too_many_arguments)]
pub fn run_test_manual_with_send(
    test_name: &str,
    options: &HashMap<&str, String>,
    #[cfg(feature = "Trace")] mut fsm: Box<Fsm>,
    #[cfg(not(feature = "Trace"))] fsm: Box<Fsm>,
    include_paths: &Vec<PathBuf>,
    #[cfg(feature = "Trace")] trace_mode: TraceMode,
    timeout: u64,
    expected_final_configuration: &Vec<String>,
    mut cb: impl FnMut(Sender<Box<Event>>),
) -> bool {
    #[cfg(feature = "Trace")]
    fsm.tracer.enable_trace(trace_mode);

    let mut executor = FsmExecutor::new_without_io_processor();
    executor.set_global_options_from_arguments(options);

    let executor_state = executor.state.clone();
    for ip in include_paths {
        executor.include_paths.push(ip.clone());
    }
    let session = fsm::start_fsm_with_data_and_finish_mode(
        fsm,
        ActionWrapper::new(),
        Box::new(executor),
        &Vec::new(),
        FinishMode::KEEP_CONFIGURATION,
    );

    let mut watchdog_sender: Option<Box<Sender<String>>> = None;
    if timeout > 0 {
        watchdog_sender = Some(start_watchdog(test_name, timeout));
    }

    // Sending some event
    cb(session.sender);

    info!("FSM started. Waiting to terminate...");
    if session.thread.is_none() {
        panic!("Internal error: session_thread not available")
    }
    let _ = session.thread.unwrap().join();

    match &watchdog_sender {
        Some(sender) => {
            // Inform watchdog
            disable_watchdog(sender);
        }
        None => {}
    }

    if expected_final_configuration.is_empty() {
        true
    } else {
        match executor_state
            .lock()
            .unwrap()
            .sessions
            .get(&session.session_id)
        {
            None => {
                error!("FSM Session lost");
                false
            }
            Some(session) => match &session.global_data.lock().unwrap().final_configuration {
                None => {
                    error!("Final Configuration not available");
                    false
                }
                Some(final_configuration) => {
                    match verify_final_configuration(expected_final_configuration, final_configuration) {
                        Ok(states) => {
                            info!(
                                "[{}] ==> Final configuration '{}' reached",
                                test_name, states
                            );
                            true
                        }
                        Err(states) => {
                            error!(
                                "[{}] ==> Expected final state '{}' not reached. Final configuration: {}",
                                test_name,
                                states,
                                final_configuration.join(",")
                            );
                            false
                        }
                    }
                }
            },
        }
    }
}

pub fn start_watchdog(test_name: &str, timeout: u64) -> Box<Sender<String>> {
    let (watchdog_sender, watchdog_receiver) = mpsc::channel();
    let test_name = test_name.to_string();

    let _timer = thread::spawn(move || {
        match watchdog_receiver.recv_timeout(Duration::from_millis(timeout)) {
            Ok(_r) => {
                // All ok, FSM terminated in time.
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                // Disconnected, also ok
            }
            Err(mpsc::RecvTimeoutError::Timeout) => abort_test(format!(
                "[{}] ==> FSM timed out after {} milliseconds",
                test_name, timeout
            )),
        }
    });
    Box::new(watchdog_sender)
}

/// Informs the watchdog that the test has finished.
///
/// + watchdog_sender - the sender-channel to the watchdog.
pub fn disable_watchdog(watchdog_sender: &Sender<String>) {
    match watchdog_sender.send("finished".to_string()) {
        Ok(_) => {}
        Err(err) => {
            warn!("Failed to send notification to watchdog. {}", err)
        }
    }
}

/// Verifies that the configuration contains a number of expected states
///
/// + expected_states - List of expected states, the FSM configuration must contain all of them.
/// + fsm_config - The final FSM configuration to verify. May contain more than the required states.
pub fn verify_final_configuration(expected_states: &Vec<String>, fsm_config: &[String]) -> Result<String, String> {
    for fc_name in expected_states {
        if !fsm_config.contains(fc_name) {
            return Err(fc_name.clone());
        }
    }
    Ok(expected_states.join(","))
}

/// Aborts the test with 1 exit code.\
/// Never returns.
pub fn abort_test(message: String) -> ! {
    error!("Fatal Error: {}", message);
    process::exit(1);
}
