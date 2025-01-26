use rufsm::actions::{Action, ActionWrapper};
use rufsm::common::init_logging;
use rufsm::datamodel::Data;
use rufsm::fsm::GlobalData;
use rufsm::fsm_executor::FsmExecutor;
use std::process::exit;

#[cfg(feature = "Trace")]
use rufsm::tracer::TraceMode;

#[derive(Clone)]
pub struct MyAction {}

impl Action for MyAction {
    fn execute(&self, arguments: &[Data], _global: &GlobalData) -> Result<Data, String> {
        let mut i = 0;
        println!("MyAction called with {} arguments:", arguments.len());
        for data in arguments {
            i += 1;
            println!("\t{}: {}", i, data)
        }
        Ok(Data::Boolean(true))
    }

    fn get_copy(&self) -> Box<dyn Action> {
        Box::new(self.clone())
    }
}

/// The FSM needs tokio :)
#[tokio::main(flavor = "multi_thread")]
async fn main() {
    // if feature EnvLog is active, this will initialize env_logger.
    init_logging();

    println!(
        r#"Action Example
-----------------------------------------
The FSM will call two custom actions from different places.
These actions can be called at any element that containt expressions or executable content.
"#
    );

    // Create the wrapper to store our actions.
    let mut actions = ActionWrapper::new();

    // We register the same function with different names.
    let my_action = MyAction {};
    actions.add_action("myEnterAction", my_action.get_copy());
    actions.add_action("myLeaveAction", Box::new(my_action));

    let session;

    // Create the fsm-executor.
    // In this example we don't need any io-processor.
    // Otherwise: let mut executor = FsmExecutor::new_with_io_processor().await;
    let mut executor = FsmExecutor::new_without_io_processor();

    // Start the FSM. Executor has different alternative
    // of this execute-methode. You can load the FSM also from memory,
    // or add some data to initialize the data-model.
    match executor.execute(
        "examples/CustomActions.scxml",
        actions,
        // If Trace feature is enabled, we can trigger additional output about
        // states and transitions. See TraceMode for the different modes.
        // The Trace feature is designed to be used for external monitoring of
        // the FSM, here it will only print the state transitions.
        #[cfg(feature = "Trace")]
        TraceMode::ALL,
    ) {
        Ok(s) => {
            session = s;
        }
        Err(err) => {
            println!("Failed to execute: {}", err);
            exit(1);
        }
    };
    // The FSM now runs in some other thread.
    // We could send events to the session via session.sender.
    // As we have nothing else to do here... we wait.
    // The example fsm will terminate after some timeout.
    let _ = session.thread.unwrap().join();

    exit(0)
}
