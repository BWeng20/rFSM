# Finite State Machine (FSM) Implementation in Rust

[![Test Status](https://github.com/BWeng20/rFSM/actions/workflows/rust.yml/badge.svg)](https://github.com/BWeng20/rFSM/actions/workflows/rust.yml) [![Rust-Slippy](https://github.com/BWeng20/rFSM/actions/workflows/rust-clippy.yml/badge.svg)](https://github.com/BWeng20/rFSM/actions/workflows/rust-clippy.yml)

![logo](logo.svg)

This project implements an embeddable and extendable Harel Statechart interpreter.</br>
Multiple state machines can be read into the runtime and executed in parallel. 
Different FSMs can communicate with each other internally or externally via events.

A datamodel can be used to maintain data and execute business logic.

## SW Design

This crate implements a _State Chart XML_ (SCXML) interpreter, according to the W3C Recommendations. See https://www.w3.org/TR/scxml/</br>
For the detailed design, see [SW Design](SW_Design.md)

## Main Crate Features

The main functional feature switches of the project:

| Name                      | Description                                                                                                     | Related crates       | Impact on Size<br/>of Release Build [^1] |
|---------------------------|-----------------------------------------------------------------------------------------------------------------|----------------------|------------------------------------------|
| ECMAScriptModel           | Adds an EMCAScript datamodel implementation.                                                                    | boa_engine           | +&#160;~&#160;10.25&#160;MiB             |
| xml                       | Enables reading SCXML (xml) files.                                                                              | quick-xml, ureq, url | +&#160;~&#160;2.07&#160;MiB              |
| RfsmExpressionModel       | Adds a datamodel implementation based on the internal Expression-Engine.                                        |                      | +&#160;~&#160;0.09&#160;MiB              |
| serializer                | Support for reading/writing FSMs in a property binary format - as alternative to xml.                           |                      | +&#160;~&#160;0.1 MiB                    |
| BasicHttpEventIOProcessor | Adds an implementation of BasicHttpEventIOProcessor                                                             | rocket, ureq         | +&#160;~&#160;4.97 MiB                   |
| json-config               | The test tool can read configurations in JSON.                                                                  | serde_json           | +&#160;~&#160;0.003&#160;MiB             |
| yaml-config               | The test tool can read configurations in YAML.                                                                  | yaml-rust            | -&#160;~&#160;0.001&#160;MiB             |
| EnvLog                    | The crate "env_log" is used as "log" implementation and for internal logging. Otherwise `std::println` is used. | env_log              | +&#160;~&#160;1.21&#160;MiB              |
| TraceServer               | Enables Remote Trace Server.                                                                                    |                      | _- not finished -_                       |

[^1]: Remind, that the features share dependencies, so the resulting size of the combined features is less than the sum of the individual features.  

A minimal feature set for a MVP is 
 + RfsmExpressionModel - _Minimalistic Datamodel using an expression-language_
 + serializer - _Reads FSMs from binary `rfsm` files_.

This leads to a minimal functional binary of ~1.4 MiB in size.</br>
 

## Extended Crate Features (for test and debugging)


| Name                      | Description                                                              | Related crates                                            | Impact on Size<br/>of Release Build |
|---------------------------|--------------------------------------------------------------------------|-----------------------------------------------------------|-------------------------------------|
| Trace_Method              | Enables tracing of methods calls in the FSM.                             |                                                           | [^2]                                |
| Trace_State               | Enables tracing of state changes in the FSM.                             |                                                           | [^2]                                |
| Trace_Event               | Enables tracing of events in the FSM.                                    |                                                           | [^2]                                |
| Debug_Reader              | Enables debug output in the SCXML reader (a lot).                        |                                                           | _don't use it!_                     |
| Debug                     | Enables additional debug (to fnd errors).                                |                                                           | _don't use it!_                     |


The trace options <i>Trace_xxx</i> still needed to be activated during runtime by settings the trace-mode.
If none of the <i>Trace_xxx</i> features are used, "Tracer" module is completely removed.

[^2]: If any of the "Trace_XXXX" features is turned on, the release build will get ~ 0.03 MiB larger. 

## Tracer

The Tracer module can be used to monitor the FSM.<br/>

The default-tracer simply prints the traced actions. If the Remote-Trace-Server-Feature is enabled, a Server is 
started that can be used by remote-clients (_to be done_).

The Tracer has different flags to control what is traced, see Enum [TraceMode](src/tracer.rs) for details.

## How To Use

FSMs normally are used embedded inside other software to control some state-full workflow.<br/> 
The transitions or the state trigger operations in the business workflow .  
To bind the business logic to a state machine different approaches exits. In hard-coded FSMs methods-calls are directly
linked to the transitions or states during compile-time. Most state machine frameworks work this way.<br/>

This crate loads FSMs during runtime, so the binding to the FSM needs to be dynamical.<br/> 
SCXML defines a _Datamodel_ for this purpose.

### Datamodel

For details about the Datamodel-concept in SCXML see the W3C documentation. This lib provides some implementations of 
the Datamodel, but you can implement your own models as well.<br/>
The Datamodel in SCXML is responsible to execute scripts and formulas. Custom business logic
can be implemented this way.<br/>
For some huge project, it may be feasible to implement the Datamodel-trait with some optimized way to trigger the 
business-functionality.<br/>

For a simpler approach (without implement a full Datamodel), see "Custom Actions" below.
You can also find some examples inside folder "examples".<br/>
Each Datamodel has a unique identifier, that can be selected in the SCXML-source, so you can provide multiple model-implementation in
one binary and use them in parallel in different FSMs.

To add new data-models, use function `rufsm::fsm::register_datamodel`.

### Provided Datamodel Implementations

+ EMCAScript-Datamodel, use `datamodel="ecmascript"`. Available if feature _"ECMAScriptModel"_ is turned on.
+ The Null-Datamodel, use `datamodel="null"`
+ Internal Expression Engine Datamodel, use `datamodel="rfsm-expression"`. Available if feature _"RfsmExpressionModel"_ is turned on.

As the EMCAScript-Datamodel is based on boa-engine, it results in a huge binary. 
If you need only basic logic in your scripts, use "rfsm-expression" instead.

For details see the [Expression-Engine-Readme](src/expression_engine/README.md).

### Custom Actions

You can use the trait "Action" to add custom functions to the FSM. See the Examples for a How-To.
Each FSM instance can have a different set of Actions. Action are inherited by child-sessions.

If using ECMAScript- or RfsmExpressions-Datamodel, these actions can be called like normal methods. 
Arguments and return values will be converted from and to JavaScript/Data-values. See enum "Data" for supported data-types.

Actions have full access to the data and states of the FSM.

## Tests

For basic functions the project contains several unit tests. The current status of these tests can be seen on the
repository start page.

More complex tests are done by test scripts that executes SCXML-files provided by the W3C.</br>
Currently, the project passed all 160 of the mandatory automated tests from the W3C test-suite.
For the detailed test process see [Test Readme](test/w3c/README.md) and for the results [Test Report](test/w3c/REPORT.MD).

## To-Dos:

+ Implement the Trace-Server to bind IDEs to the runtime.


> [!NOTE]
> ### Not conformant or not implemented features of the W3C recommendation
>
> + XML inside &lt;content> is not handled according to _[content_and_namespaces](doc/W3C_SCXML_2024_07_13/index.html#content_and_namespaces)_. The content inside &lt;content> is not
    >  interpreted and send to the receiver unmodified.
> + BasicHTTP Event I/O processor doesn't set the '_event.raw' member, that is needed for optional
    >   tests 178, 509, 519, 520 and 534.
