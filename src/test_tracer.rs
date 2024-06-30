use std::{process, thread};
#[cfg(test)]
use std::{println as error, println as warn};
use std::collections::HashMap;
use std::sync::{Arc, mpsc, Mutex};
use std::sync::mpsc::Sender;
use std::time::Duration;

#[cfg(not(test))]
use log::{error, warn};

use crate::fsm::State;
use crate::tracer::{DefaultTracer, TraceMode, Tracer};

/// Aborts the test with 1 exit code.\
/// Never returns.
pub fn abort_test(message: String) -> ! {
    error!("Fatal Error: {}", message);
    process::exit(1);
}

/// Trace implementation to protocol the current states.
/// Shall be used in test to verify the final FSM configuration.
#[derive(Debug)]
pub struct TestTracer {
    current_config: Arc<Mutex<HashMap<String, String>>>,
    default_tracer: DefaultTracer,
}

impl TestTracer {
    pub fn new() -> TestTracer {
        let tracer = TestTracer {
            current_config: Arc::new(Mutex::new(HashMap::new())),
            default_tracer: DefaultTracer::new(),
        };
        tracer
    }

    pub fn get_fsm_config(&self) -> Arc<Mutex<HashMap<String, String>>> {
        self.current_config.clone()
    }


    /// Informs the watchdog that the test has finished.
    ///
    /// + watchdog_sender - the sender-channel to the watchdog.
    pub fn disable_watchdog(watchdog_sender: &Box<Sender<String>>) {
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
    pub fn verify_final_configuration(expected_states: &Vec<String>, fsm_config: &Arc<Mutex<HashMap<String, String>>>) -> Result<String, String> {
        let guard = fsm_config.lock().unwrap();
        for fc_name in expected_states {
            if !guard.contains_key(fc_name.as_str()) {
                return Err(fc_name.clone());
            }
        }
        return Ok(expected_states.join(","));
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
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    abort_test(format!("[{}] ==> FSM timed out after {} milliseconds", test_name, timeout))
                }
            }
        });
        Box::new(watchdog_sender)
    }
}

impl Tracer for TestTracer {
    fn trace(&self, msg: &str) {
        self.default_tracer.trace(msg);
    }

    fn enter(&self) {
        self.default_tracer.enter()
    }

    fn leave(&self) {
        self.default_tracer.leave()
    }

    fn enable_trace(&mut self, flag: TraceMode) {
        self.default_tracer.enable_trace(flag);
    }

    fn disable_trace(&mut self, flag: TraceMode) {
        self.default_tracer.disable_trace(flag);
    }

    fn is_trace(&self, flag: TraceMode) -> bool {
        self.default_tracer.is_trace(flag)
    }

    fn trace_enter_state(&self, s: &State) {
        let mut guard = self.current_config.lock().unwrap();
        guard.insert(s.name.clone(), s.name.clone());
        self.default_tracer.trace_enter_state(s);
    }

    fn trace_exit_state(&self, s: &State) {
        self.trace_state("Exit", s);
        let mut guard = self.current_config.lock().unwrap();
        guard.remove(s.name.as_str());
        self.default_tracer.trace_exit_state(s);
    }

    fn trace_mode(&self) -> TraceMode {
        self.default_tracer.trace_mode()
    }
}
