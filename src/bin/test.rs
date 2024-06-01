use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;
use std::process;
use std::sync::{Arc, Mutex};

#[cfg(feature = "json-config")]
use serde::Deserialize;
#[cfg(feature = "yaml-config")]
use yaml_rust::YamlLoader;

use rfsm::{fsm, reader};
use rfsm::fsm::{Fsm, State};
use rfsm::fsm_executor::ExecuterState;
use rfsm::tracer::{TraceMode, Tracer};

#[derive(Debug)]
#[cfg_attr(feature = "json-config", derive(Deserialize))]
pub struct EventSpecification {
    /// Mandatory event name to send.
    name: String,

    /// Delay in milliseconds after the event was send.
    delay_ms: i32,

    /// Optional state to reach after the event\
    /// Use "#stop" to check for termination of FSM.
    shall_reach_state: Option<String>,

    /// Optional event to receive from FSM after the event.
    shall_send_event: Option<String>,
}

#[derive(Debug)]
#[cfg_attr(feature = "json-config", derive(Deserialize))]
pub struct TestSpecification {
    file: Option<String>,
    events: Vec<EventSpecification>,
    final_configuration: Option<Vec<String>>,
}

#[derive(Debug)]
pub struct TestUseCase {
    name: String,
    specification: TestSpecification,
    fsm: Option<Box<Fsm>>,
}

#[tokio::main(flavor = "multi_thread")]
async fn main() {
    #[cfg(feature = "EnvLog")]
    env_logger::init();

    let (named_opt, final_args) = rfsm::get_arguments(&[
        TraceMode::argument_option()
    ]);

    let trace = TraceMode::from_arguments(&named_opt);

    if final_args.len() < 1 {
        abort_test("Missing argument. Please specify one or more test file(s)".to_string());
    }

    let mut test_spec_file = "".to_string();
    let mut config: Option<TestSpecification> = Option::None;
    let mut fsm: Option<Box<Fsm>> = None;

    for arg in final_args {
        let ext: String;
        match Path::new(arg.as_str()).extension() {
            None => {
                ext = String::new()
            }
            Some(oext) => {
                ext = oext.to_string_lossy().to_string();
            }
        }
        match ext.to_lowercase().as_str() {
            "yaml" | "yml" => {
                #[cfg(feature = "yaml-config")]
                {
                    config = Some(load_yaml_config(arg.as_str()));
                    test_spec_file = arg.clone();
                }
                #[cfg(not(feature = "yaml-config"))]
                {
                    abort_test(format!("feature 'yaml-config' is not configured. Can't load '{}'", arg));
                }
            }
            "json" | "js" => {
                #[cfg(feature = "json-config")]
                {
                    config = Some(load_json_config(arg.as_str()));
                    test_spec_file = arg.clone();
                }
                #[cfg(not(feature = "json-config"))]
                {
                    abort_test(format!("feature 'json-config' is not configured. Can't load '{}'", arg));
                }
            }
            "scxml" | "xml" => {
                match load_fsm(arg.as_str()) {
                    Ok(mut fsm_loaded) => {
                        fsm_loaded.tracer.enable_trace(trace);
                        fsm = Some(fsm_loaded);
                    }
                    Err(_) => {
                        abort_test(format!("Failed to load fsm '{}'", arg).to_string())
                    }
                }
            }
            &_ => {
                abort_test(format!("File '{}' has unsupported extension.", arg).to_string())
            }
        }
    }
    match config {
        Some(mut test_spec) => {
            let uc = TestUseCase {
                fsm: if test_spec.file.is_some()
                {
                    if fsm.is_some() {
                        abort_test(format!("Test Specification '{}' contains a fsm path, but program arguments define some other fsm",
                                           test_spec_file).to_string())
                    }
                    test_spec_file = test_spec.file.clone().unwrap();
                    match load_fsm(test_spec_file.as_str()) {
                        Ok(mut fsm) => {
                            fsm.tracer.enable_trace(trace);
                            println!("Loaded {}", test_spec_file);
                            Option::Some(fsm)
                        }
                        Err(_err) => {
                            abort_test(format!("Failed to load fsm '{}'", test_spec_file).to_string());
                        }
                    }
                } else {
                    fsm
                },
                specification: test_spec,
                name: test_spec_file,
            };
            run_test(uc);
        }
        None => {
            abort_test("No test specification given.".to_string());
        }
    }
}

pub fn load_fsm(file_path: &str) -> Result<Box<Fsm>, String> {
    reader::read_from_xml_file(file_path.to_string())
}

#[cfg(feature = "yaml-config")]
pub fn load_yaml_config(file_path: &str) -> TestSpecification {
    match File::open(file_path) {
        Ok(file) => {
            let mut reader = BufReader::new(file);

            let mut yaml = String::new();
            match reader.read_to_string(&mut yaml) {
                Ok(_) => {
                    match YamlLoader::load_from_str(&yaml) {
                        Ok(doc) => {
                            todo!()
                        }
                        Err(err) => {
                            abort_test(format!("Error de-serializing config file '{}'. {}", file_path, err));
                        }
                    }
                }
                Err(err) => {
                    abort_test(format!("Error reading config file '{}'. {}", file_path, err));
                }
            }
        }
        Err(err) => {
            abort_test(format!("Error reading config file '{}'. {}", file_path, err));
        }
    }
}

#[cfg(feature = "json-config")]
pub fn load_json_config(file_path: &str) -> TestSpecification {
    match File::open(file_path) {
        Ok(file) => {
            let reader = BufReader::new(file);
            match serde_json::from_reader::<BufReader<File>, TestSpecification>(reader) {
                Ok(test) => {
                    test
                }
                Err(err) => {
                    abort_test(format!("Error de-serializing config file '{}'. {}", file_path, err));
                }
            }
        }
        Err(err) => {
            abort_test(format!("Error reading config file '{}'. {}", file_path, err));
        }
    }
}

#[derive(Debug)]
pub struct TestTracer {
    current_config: Arc<Mutex<HashMap<String, String>>>,
}

impl TestTracer {
    pub fn new(config: Arc<Mutex<HashMap<String, String>>>) -> TestTracer {
        TestTracer {
            current_config: config
        }
    }
}

impl Tracer for TestTracer {
    fn trace(&self, msg: &str) {
        println!("{}", msg);
    }

    fn enter(&self) {}

    fn leave(&self) {}

    fn enable_trace(&mut self, flag: TraceMode) {}

    fn disable_trace(&mut self, flag: TraceMode) {}

    fn is_trace(&self, flag: TraceMode) -> bool {
        true
    }

    fn trace_enter_state(&self, s: &State) {
        self.trace_state("Enter", s);
        let mut guard = self.current_config.lock().unwrap();
        guard.insert(s.name.clone(), s.name.clone());
    }

    fn trace_exit_state(&self, s: &State) {
        self.trace_state("Exit", s);
        let mut guard = self.current_config.lock().unwrap();
        guard.remove(s.name.as_str());
    }
}

pub fn run_test(test: TestUseCase) {
    println!("{:?}", test);

    if test.fsm.is_none() {
        abort_test(format!("No FSM given in test '{}'", test.name))
    }

    let current_config = Arc::new(Mutex::new(HashMap::new()));


    let mut fsm = test.fsm.unwrap();
    fsm.tracer = Box::new(TestTracer::new(current_config.clone()));
    let state = Arc::new(Mutex::new(ExecuterState::new()));
    let (thread_join, sender) = fsm::start_fsm(fsm, &state);

    println!("FSM started. Waiting to terminate...");
    let _ = thread_join.join();

    let mut guard = current_config.lock().unwrap();
    println!("Final Configuration {:?}", guard.values());

    match test.specification.final_configuration {
        None => {}
        Some(fc) => {
            for fc_name in fc {
                if guard.contains_key(fc_name.as_str()) {
                    println!("[{}] ==> Final state '{}' reached", test.name, fc_name);
                } else {
                    abort_test(format!("[{}] ==> Expected final state '{}' not reached", test.name, fc_name));
                }
            }
        }
    }


    process::exit(0);
}

/// Aborts the test with 1 exit code.\
/// Never returns.
pub fn abort_test(message: String) -> ! {
    println!("Fatal Error: {}", message);
    process::exit(1);
}