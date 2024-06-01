use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;
use std::process;

#[cfg(feature = "json-config")]
use serde::Deserialize;
use yaml_rust::YamlLoader;
use rfsm::fsm::Fsm;
use rfsm::tracer::TraceMode;

#[derive( Debug)]
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
    shall_send_event:  Option<String>
}

#[derive(Deserialize, Debug)]
pub struct TestSpecification {
    file: Option<String>,
    events: Vec<EventSpecification>,
    final_configuration: Option<Vec<String>>
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
        println!("Missing argument. Please specify one or more test file(s)");
        process::exit(1);
    }

    let mut config= Result::Err(());
    let mut fsm= Result::Err(());

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
            #[cfg(feature = "yaml-config")]
            "yaml" | "yml" => {
                config = load_yaml_config(arg.as_str());
            }
            #[cfg(feature = "json-config")]
            "json" | "js" => {
                config = load_json_config(arg.as_str());
            }
            "scxml" | "xml" => {
                fsm = load_fsm( arg.as_str() );
            }
            &_ => {
                println!("File '{}' has unsupported extension.", arg);
                process::exit(1);
            }
        }
    }
    match config {
        Ok(test_spec) => {
            run_test(test_spec);
        }
        Err(err) => {
            println!("Error configuration file.");
            process::exit(1);
        }
    }
}

pub fn load_fsm( file_path : &str ) -> Result<Fsm,()> {
    todo!();
}

#[cfg(feature = "yaml-config")]
pub fn load_yaml_config( file_path : &str ) -> Result<TestSpecification,()> {
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
                            println!("Error parsing config file '{}'. {}", file_path, err);
                            return Result::Err(());
                        }
                    }
                }
                Err(err) => {
                    println!("Error reading config file '{}'. {}", file_path, err);
                    return Result::Err(());
                }
            }

        }
        Err(err) => {
            println!("Error reading file. {}", err);
            return Result::Err(());
        }
    }
}

#[cfg(feature = "json-config")]
pub fn load_json_config( file_path : &str ) -> Result<TestSpecification,()> {
    match File::open(file_path) {
        Ok(file) => {
            let mut reader = BufReader::new(file);
            match serde_json::from_reader::<BufReader<File>, TestSpecification>(reader) {
                Ok(test) => {
                    return Result::Ok(test);
                }
                Err(err) => {
                    return Result::Err(());
                }
            }
        }
        Err(err) => {
            println!("Error reading config file. {}", err);
            return Result::Err(());
        }
    }
}

pub fn run_test(mut test: TestSpecification) {
    println!("{:?}", test);

    todo!()
}