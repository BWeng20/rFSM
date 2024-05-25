use std::ffi::OsStr;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;
use std::process;

#[cfg(feature = "json-config")]
use serde::Deserialize;
use yaml_rust::{ScanError, Yaml, YamlLoader};

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
    file: String,
    events: Vec<EventSpecification>,
}

#[tokio::main(flavor = "multi_thread")]
async fn main() {
    #[cfg(feature = "EnvLog")]
    env_logger::init();

    let (_trace, final_args) = rfsm::get_arguments();

    if final_args.len() < 1 {
        println!("Missing argument. Please specify one or more test file(s)");
        process::exit(1);
    }

    run_test_file(final_args[0].as_str());
}

pub fn run_test_file(file_path : &str) {
    match File::open(file_path) {
        Ok(file) => {
            let mut reader = BufReader::new(file);

            let ext : String;
            match Path::new(file_path).extension() {
                None => {
                    ext =  String::new()
                }
                Some(oext) => {
                    ext = oext.to_string_lossy().to_string();
                }
            }
            match ext.to_lowercase().as_str() {
                #[cfg(feature = "yaml-config")]
                "yaml" | "yml" =>
                {
                    let mut yaml = String::new();
                    match reader.read_to_string(&mut yaml) {
                        Ok(_) => {
                            match YamlLoader::load_from_str(&yaml) {
                                Ok(doc) => {
                                    todo!()
                                }
                                Err(err) => {
                                    println!("Error parsing test file '{}'. {}", file_path, err);
                                    process::exit(1);
                                }
                            }
                        }
                        Err(err) => {
                            println!("Error reading test file '{}'. {}", file_path, err);
                            process::exit(1);
                        }
                    }
                }
                #[cfg(feature = "json-config")]
                "json" =>
                {
                    let v = serde_json::from_reader::<BufReader<File>, TestSpecification>(reader);
                    match v {
                        Ok(test_spec) => {
                            run_test(test_spec);
                        }
                        Err(err) => {
                            println!("Error deserializing test file '{}'. {}", file_path, err);
                            process::exit(1);
                        }
                    }
                }
                _ => {
                    println!("File '{}' has unsupported extention.", file_path );
                    process::exit(1);
                }
            }
        }
        Err(err) => {
            println!("Error reading test file. {}", err);
            process::exit(2);
        }
    }
}

pub fn run_test(mut test: TestSpecification) {
    println!("{:?}", test);

    todo!()
}