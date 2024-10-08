extern crate core;

use log::error;
use rfsm::scxml_reader::include_path_from_arguments;
use rfsm::scxml_reader::INCLUDE_PATH_ARGUMENT_OPTION;
use rfsm::serializer::default_protocol_writer::DefaultProtocolWriter;
use rfsm::serializer::fsm_writer::FsmWriter;
use rfsm::{init_logging, scxml_reader};
use std::fs::File;
use std::io::BufWriter;
use std::process;

#[tokio::main(flavor = "multi_thread")]
async fn main() {
    init_logging();

    let (named_opt, final_args) = rfsm::get_arguments(&[&INCLUDE_PATH_ARGUMENT_OPTION]);

    if final_args.len() < 2 {
        println!("Missing argument. Please specify scxml-input- and fsm-output-file");
        process::exit(1);
    }

    let source_file = final_args[0].clone();
    let target_file = final_args[1].clone();

    let include_paths = include_path_from_arguments(&named_opt);
    println!("Reading from {}", source_file);
    match scxml_reader::parse_from_uri(source_file, &include_paths) {
        Ok(fsm) => match File::create(target_file.clone()) {
            Ok(f) => {
                println!("Writing to {}", &target_file);
                let protocol = DefaultProtocolWriter::new(BufWriter::new(f));
                let mut writer = FsmWriter::new(Box::new(protocol));
                writer.write(&fsm);
                writer.close();
            }
            Err(err) => {
                error!("Failed to open output: {}", err);
                process::exit(2);
            }
        },
        Err(err) => {
            error!("Failed to load SCXML:{}", err);
            process::exit(2);
        }
    }
}
