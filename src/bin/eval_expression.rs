//! Demonstration and Test of ruFsm-Expressions.
//! Usage:
//!    eval \<expressions\>

use rufsm::common::info;
use std::process;

use rufsm::common::init_logging;
use rufsm::datamodel::create_global_data_arc;
use rufsm::datamodel::expression_engine::RFsmExpressionDatamodel;
use rufsm::expression_engine::parser::ExpressionParser;

fn main() {
    init_logging();

    let (_named_opt, final_args) = rufsm::common::get_arguments(&[]);

    if final_args.is_empty() {
        info!("Missing argument. Please specify one ruFsm Expression");
        process::exit(1);
    }

    let ec = RFsmExpressionDatamodel::new(create_global_data_arc());

    for s in final_args {
        let rs = ExpressionParser::execute_str(s.as_str(), &mut ec.global_data.lock().unwrap());

        match rs {
            Ok(r) => {
                println!("Result: {}", r);
            }
            Err(err) => {
                println!("Error {}", err);
            }
        }
    }
}
