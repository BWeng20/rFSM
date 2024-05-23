use std::fs::File;
use std::io::BufReader;
use std::process;

use serde::Deserialize;

#[derive(Deserialize, Debug)]
pub struct TestSpecification {
    file: String,
    events: Vec<String>,
}

#[tokio::main(flavor = "multi_thread")]
async fn main() {
    #[cfg(feature = "EnvLog")]
    env_logger::init();

    let (_trace, final_args) = rfsm::get_arguments();

    if final_args.len() < 1 {
        println!("Missing argument. Please specify one or more test file");
        process::exit(1);
    }

    let file_path = final_args[0].as_str();
    match File::open(file_path) {
        Ok(file) => {
            let reader = BufReader::new(file);
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
        Err(err) => {
            println!("Error reading test file. {}", err);
            process::exit(1);
        }
    }
}

pub fn run_test(mut test: TestSpecification) {
    println!("{:?}", test);

    todo!()
}