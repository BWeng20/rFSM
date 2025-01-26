use std::path::Path;

#[cfg(feature = "Debug")]
use rufsm::common::debug;
use rufsm::fsm::Fsm;
#[cfg(feature = "xml")]
use rufsm::scxml_reader;
#[cfg(feature = "xml")]
use rufsm::scxml_reader::INCLUDE_PATH_ARGUMENT_OPTION;
#[cfg(feature = "json-config")]
use rufsm::test::load_json_config;
#[cfg(feature = "yaml-config")]
use rufsm::test::load_yaml_config;
use rufsm::test::{abort_test, load_fsm, run_test, TestSpecification, TestUseCase};
#[cfg(feature = "Trace")]
use rufsm::tracer::{TraceMode, TRACE_ARGUMENT_OPTION};

#[cfg(feature = "Debug")]
use rufsm::common::get_features;
use rufsm::common::init_logging;

#[cfg(feature = "TraceServer")]
use rufsm::remote_tracer::run_trace_server;
#[cfg(feature = "TraceServer")]
use rufsm::remote_tracer::TRACE_SERVER_ARGUMENT_OPTION;

#[tokio::main(flavor = "multi_thread")]
async fn main() {
    init_logging();

    #[cfg(feature = "Debug")]
    {
        println!("Active features:");
        let features = get_features();
        for s in features {
            println!("\t{}", s)
        }
    }

    #[allow(unused_variables)]
    let (named_opt, final_args) = rufsm::common::get_arguments(&[
        #[cfg(feature = "Trace")]
        &TRACE_ARGUMENT_OPTION,
        #[cfg(feature = "TraceServer")]
        &TRACE_SERVER_ARGUMENT_OPTION,
        #[cfg(feature = "xml")]
        &INCLUDE_PATH_ARGUMENT_OPTION,
    ]);

    #[cfg(feature = "Trace")]
    let trace = TraceMode::from_arguments(&named_opt);

    #[cfg(feature = "TraceServer")]
    {
        match named_opt.get(TRACE_SERVER_ARGUMENT_OPTION.name) {
            None => {}
            Some(trace_server_adr) => {
                let _trace_thread = run_trace_server(trace_server_adr.as_str()).await;
            }
        }
    }

    #[cfg(feature = "xml")]
    let include_paths = scxml_reader::include_path_from_arguments(&named_opt);
    #[cfg(not(feature = "xml"))]
    let include_paths = Vec::new();

    if final_args.is_empty() {
        abort_test("Missing argument. Please specify one or more test file(s)".to_string());
    }

    let mut test_spec_file = "".to_string();
    #[allow(unused_mut)]
    let mut config: Option<TestSpecification> = None;
    let mut fsm: Option<Box<Fsm>> = None;

    for arg in &final_args {
        let ext = match Path::new(arg.as_str()).extension() {
            None => String::new(),
            Some(oext) => oext.to_string_lossy().to_string(),
        };
        match ext.to_lowercase().as_str() {
            "yaml" | "yml" => {
                #[cfg(feature = "yaml-config")]
                {
                    config = Some(load_yaml_config(arg.as_str()));
                    test_spec_file.clone_from(arg);
                }
                #[cfg(not(feature = "yaml-config"))]
                {
                    abort_test(format!(
                        "feature 'yaml-config' is not configured. Can't load '{}'",
                        arg
                    ));
                }
            }
            "json" | "js" => {
                #[cfg(feature = "json-config")]
                {
                    config = Some(load_json_config(arg.as_str()));
                    test_spec_file.clone_from(arg);
                }
                #[cfg(not(feature = "json-config"))]
                {
                    abort_test(format!(
                        "feature 'json-config' is not configured. Can't load '{}'",
                        arg
                    ));
                }
            }
            "rfsm" | "scxml" | "xml" => match load_fsm(arg.as_str(), &include_paths) {
                Ok(fsm_loaded) => {
                    fsm = Some(fsm_loaded);
                }
                Err(err) => abort_test(format!("Failed to load fsm '{}'. {}", arg, err).to_string()),
            },
            &_ => abort_test(format!("File '{}' has unsupported extension.", arg).to_string()),
        }
    }
    match config {
        Some(test_spec) => {
            let uc = TestUseCase {
                fsm: if test_spec.file.is_some() {
                    if fsm.is_some() {
                        abort_test(format!("Test Specification '{}' contains a fsm path, but program arguments define some other fsm",
                                           test_spec_file).to_string())
                    }
                    test_spec_file = test_spec.file.clone().unwrap();
                    match load_fsm(test_spec_file.as_str(), &include_paths) {
                        #[allow(unused_mut)]
                        Ok(mut fsm) => {
                            #[cfg(feature = "Trace")]
                            fsm.tracer.enable_trace(trace);
                            #[cfg(feature = "Debug")]
                            debug!("Loaded {}", test_spec_file);
                            Some(fsm)
                        }
                        Err(_err) => abort_test(format!("Failed to load fsm '{}'", test_spec_file).to_string()),
                    }
                } else {
                    fsm
                },
                specification: test_spec,
                name: test_spec_file,
                #[cfg(feature = "Trace")]
                trace_mode: trace,
                include_paths: include_paths.clone(),
            };
            run_test(uc);
        }
        None => {
            abort_test("No test specification given.".to_string());
        }
    }
}
