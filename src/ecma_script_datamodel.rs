//! Implements the SCXML Data model for ECMA with Boa Engine.\
//! Included if feature "ECMAScript" is enabled.\
//! See [W3C:The ECMAScript Data Model](/doc/W3C_SCXML_2024_07_13/index.html#ecma-profile).\
//! See [GitHub:Boa Engine](https://github.com/boa-dev/boa).

use std::collections::HashMap;
use std::str::FromStr;
use std::string::ToString;
#[cfg(test)]
use std::{println as debug, println as info, println as warn, println as error};

use crate::ArgOption;
use boa_engine::context::ContextBuilder;
use boa_engine::object::builtins::JsMap;
use boa_engine::object::ObjectInitializer;
use boa_engine::property::{Attribute, PropertyDescriptor};
use boa_engine::value::Type;
use boa_engine::{js_string, native_function::NativeFunction, Context, JsError, JsValue, Source};
use boa_engine::{JsArgs, JsData, JsResult};
use boa_gc::{empty_trace, Finalize, Trace};

#[cfg(not(test))]
use log::{debug, error, info, warn};

use crate::datamodel::{
    Data, DataStore, Datamodel, GlobalDataAccess, EVENT_VARIABLE_FIELD_DATA,
    EVENT_VARIABLE_FIELD_INVOKE_ID, EVENT_VARIABLE_FIELD_NAME, EVENT_VARIABLE_FIELD_ORIGIN,
    EVENT_VARIABLE_FIELD_ORIGIN_TYPE, EVENT_VARIABLE_FIELD_SEND_ID, EVENT_VARIABLE_FIELD_TYPE,
    EVENT_VARIABLE_NAME,
};
use crate::event_io_processor::{EventIOProcessor, SYS_IO_PROCESSORS};
use crate::executable_content::{
    DefaultExecutableContentTracer, ExecutableContent, ExecutableContentTracer,
};
use crate::fsm::{Event, ExecutableContentId, Fsm, State, StateId};

pub const ECMA_SCRIPT: &str = "ECMAScript";
pub const ECMA_SCRIPT_LC: &str = "ecmascript";

pub const ECMA_OPTION_INFIX: &str = "ecma:";
pub const ECMA_OPTION_STRICT_POSTFIX: &str = "strict";

pub const ECMA_STRICT_OPTION: &str = "datamodel:ecma:strict";

pub static ECMA_STRICT_ARGUMENT: ArgOption = ArgOption {
    name: ECMA_STRICT_OPTION,
    with_value: false,
    required: false,
};

pub struct ECMAScriptDatamodel {
    pub data: DataStore,
    pub global_data: GlobalDataAccess,
    pub context: Context,
    pub tracer: Option<Box<dyn ExecutableContentTracer>>,
    pub io_processors: HashMap<String, Box<dyn EventIOProcessor>>,
    pub strict_mode: bool,
}

fn js_to_string(jv: &JsValue, ctx: &mut Context) -> String {
    match jv.to_string(ctx) {
        Ok(s) => s.to_std_string().unwrap().clone(),
        Err(_e) => jv.display().to_string(),
    }
}

fn option_to_js_value(val: &Option<String>) -> JsValue {
    match val {
        Some(s) => JsValue::from(js_string!(s.clone())),
        None => JsValue::Undefined,
    }
}

#[derive(JsData, Finalize)]
struct FsmJSWrapper {
    pub global_data: GlobalDataAccess,
    pub state_name_to_id: HashMap<String, StateId>,
}

/// Dummy implementation for the Wrapper to enable us to add this class to the context.\
/// Safety: Nothing in this struct needs tracing, so this is safe.
unsafe impl Trace for FsmJSWrapper {
    empty_trace!();
}

impl FsmJSWrapper {
    fn new(gd: GlobalDataAccess) -> FsmJSWrapper {
        FsmJSWrapper {
            global_data: gd,
            state_name_to_id: HashMap::new(),
        }
    }
}

impl ECMAScriptDatamodel {
    pub fn new(global_data: GlobalDataAccess) -> ECMAScriptDatamodel {
        ECMAScriptDatamodel {
            data: DataStore::new(),
            global_data,
            context: ContextBuilder::new().build().unwrap(),
            tracer: Some(Box::new(DefaultExecutableContentTracer::new())),
            io_processors: HashMap::new(),
            strict_mode: false,
        }
    }

    pub fn set_option(&mut self, name: &str, _value: &str) {
        if let Some(ecma_option) = name.strip_prefix(ECMA_OPTION_INFIX) {
            match ecma_option {
                ECMA_OPTION_STRICT_POSTFIX => {
                    info!("Running ECMA in strict mode");
                    self.strict_mode = true;
                    self.context.strict(true);
                }
                &_ => {}
            }
        }
    }

    pub fn set_from_data_store(&mut self, data: &DataStore, set_data: bool) {
        for (name, data) in &data.values {
            if set_data {
                match &data.value {
                    None => {
                        self.set_js_property(name.as_str(), JsValue::Null);
                    }
                    Some(dv) => {
                        let rs = self.context.eval(Source::from_bytes(dv.as_str()));
                        match rs {
                            Ok(val) => {
                                self.set_js_property(name.as_str(), val);
                            }
                            Err(err) => {
                                error!("Error on Initialize '{}': {}", name, err);
                                // W3C says:
                                // If the value specified for a <data> element (by 'src', children, or
                                // the environment) is not a legal data value, the SCXML Processor MUST
                                // raise place error.execution in the internal event queue and MUST
                                // create an empty data element in the data model with the specified id.
                                self.set_js_property(name.as_str(), JsValue::Undefined);
                                self.internal_error_execution();
                            }
                        }
                    }
                }
            } else {
                self.set_js_property(name.as_str(), JsValue::Undefined);
            }
        }
    }

    fn execute_internal(&mut self, script: &str, handle_error: bool) -> Result<String, String> {
        let result = self.eval(script);
        match result {
            Ok(res) => {
                if res.is_undefined() {
                    debug!("Execute: {} => undefined", script);
                    Ok("".to_string())
                } else {
                    match res.to_string(&mut self.context) {
                        Ok(str) => {
                            let r = str.to_std_string_escaped();
                            debug!("Execute: {} => {}", script, r);
                            Ok(r)
                        }
                        Err(err) => {
                            let msg = format!(
                                "Script Error - failed to convert result to string: {} => {}",
                                script, err
                            );
                            warn!("{}", msg);
                            if handle_error {
                                self.internal_error_execution();
                            }
                            Err(msg)
                        }
                    }
                }
            }
            Err(e) => {
                // Pretty print the error
                let msg = format!("Script Error:  {} => {} ", script, e);
                error!("{}", msg);
                Err(msg)
            }
        }
    }

    fn execute_content(&mut self, fsm: &Fsm, e: &dyn ExecutableContent) -> bool {
        match &mut self.tracer {
            Some(t) => {
                e.trace(t.as_mut(), fsm);
            }
            None => {}
        }
        e.execute(self, fsm)
    }

    fn eval(&mut self, source: &str) -> JsResult<JsValue> {
        self.context.eval(Source::from_bytes(source))
    }

    fn set_js_property<V>(&mut self, name: &str, value: V)
    where
        V: Into<JsValue>,
    {
        _ = self
            .context
            .global_object()
            .set(js_string!(name), value, false, &mut self.context);
    }

    fn assign_internal(
        self: &mut ECMAScriptDatamodel,
        left_expr: &str,
        right_expr: &str,
        allow_undefined: bool,
    ) -> bool {
        let exp = format!("{}={}", left_expr, right_expr);
        if allow_undefined && self.strict_mode {
            self.context.strict(false);
        }
        let r = match self.eval(exp.as_str()) {
            Ok(_) => true,
            Err(error) => {
                // W3C says:\
                // If the location expression does not denote a valid location in the data model or
                // if the value specified (by 'expr' or children) is not a legal value for the
                // location specified, the SCXML Processor must place the error 'error.execution'
                // in the internal event queue.
                self.log(
                    format!(
                        "Could not assign {}={}, '{}'.",
                        left_expr, right_expr, error
                    )
                    .as_str(),
                );

                self.internal_error_execution();
                false
            }
        };
        if allow_undefined && self.strict_mode {
            self.context.strict(true);
        }
        r
    }

    fn in_configuration(
        _this: &JsValue,
        args: &[JsValue],
        context: &mut Context,
    ) -> JsResult<JsValue> {
        let state = args.get_or_undefined(0);

        if let Ok(name) = state.to_string(context) {
            let fsm = context.get_data::<FsmJSWrapper>().unwrap();
            let loc = fsm.state_name_to_id.get(&name.to_std_string().unwrap());
            if fsm
                .global_data
                .lock()
                .configuration
                .data
                .contains(loc.unwrap())
            {
                return Ok(JsValue::Boolean(true));
            }
        }
        Ok(JsValue::Boolean(false))
    }

    fn log_js(_this: &JsValue, args: &[JsValue], ctx: &mut Context) -> Result<JsValue, JsError> {
        let mut msg = String::new();
        for arg in args {
            msg.push_str(js_to_string(arg, ctx).as_str());
        }
        info!("{}", msg);
        Ok(JsValue::Null)
    }
}

impl Datamodel for ECMAScriptDatamodel {
    fn global(&mut self) -> &mut GlobalDataAccess {
        &mut self.global_data
    }
    fn global_s(&self) -> &GlobalDataAccess {
        &self.global_data
    }

    fn get_name(&self) -> &str {
        ECMA_SCRIPT
    }

    fn implement_mandatory_functionality(&mut self, fsm: &mut Fsm) {
        let session_id = self.global().lock().session_id;
        let ctx = &mut self.context;

        // Implement "In" function.
        let _ = ctx.register_global_callable(
            js_string!("__In"),
            1,
            NativeFunction::from_copy_closure(Self::in_configuration),
        );

        let mut fw = FsmJSWrapper::new(self.global_data.clone());
        for state in fsm.states.as_slice() {
            fw.state_name_to_id.insert(state.name.clone(), state.id);
        }

        let _ = ctx.insert_data(fw);

        let _ = ctx.eval(Source::from_bytes(
            r##"
                function In(state) {
                   return __In( state );
                }
            "##,
        ));

        // Implement "log" function.
        let _ = ctx.register_global_callable(
            js_string!("log"),
            1,
            NativeFunction::from_copy_closure(Self::log_js),
        );

        // set system variable "_ioprocessors"
        {
            // Create I/O-Processor Objects.
            let io_processors_js = JsMap::new(ctx);
            for (name, processor) in &self.io_processors {
                let processor_js = JsMap::new(ctx);
                let location = js_string!(processor.get_location(session_id));
                _ = processor_js.create_data_property(js_string!("location"), location, ctx);
                // @TODO
                _ = io_processors_js.create_data_property(
                    js_string!(name.as_str()),
                    processor_js,
                    ctx,
                );
            }
            let r = self.context.global_object().define_property_or_throw(
                js_string!(SYS_IO_PROCESSORS),
                PropertyDescriptor::builder()
                    .configurable(true)
                    .enumerable(false)
                    .writable(false)
                    .value(io_processors_js),
                &mut self.context,
            );
            if let Err(error) = r {
                error!("Failed to initialize {}: {}", SYS_IO_PROCESSORS, error);
            }
        }
    }

    #[allow(non_snake_case)]
    fn initializeDataModel(&mut self, fsm: &mut Fsm, data_state: StateId, set_data: bool) {
        let state_obj: &State = fsm.get_state_by_id_mut(data_state);
        // Set all (simple) global variables.
        self.set_from_data_store(&state_obj.data, set_data);
        if data_state == fsm.pseudo_root {
            let mut ds = DataStore::new();
            ds.values = self.global_data.lock().environment.values.clone();
            self.set_from_data_store(&ds, true);
        }
    }

    fn initialize_read_only(&mut self, name: &str, value: &str) {
        let r = self.context.global_object().define_property_or_throw(
            js_string!(name),
            PropertyDescriptor::builder()
                .configurable(true)
                .enumerable(false)
                .writable(false)
                .value(js_string!(value)),
            &mut self.context,
        );
        if let Err(error) = r {
            error!("Failed to initialize read only {}: {}", name, error);
        }
    }

    fn set(self: &mut ECMAScriptDatamodel, name: &str, data: Data) {
        let str_val = data.to_string().clone();
        self.data.set(name, data);
        self.set_js_property(name, js_string!(str_val));
    }

    fn set_event(&mut self, event: &crate::fsm::Event) {
        let data_value = match &event.param_values {
            None => match &event.content {
                None => JsValue::Undefined,
                Some(c) => JsValue::String(js_string!(c.clone())),
            },
            Some(pv) => {
                let mut data_object_initializer = ObjectInitializer::new(&mut self.context);
                for pair in pv.iter() {
                    data_object_initializer.property(
                        js_string!(pair.name.clone()),
                        js_string!(pair.value.to_string()),
                        Attribute::all(),
                    );
                }
                JsValue::Object(data_object_initializer.build())
            }
        };

        let mut event_object_initializer = ObjectInitializer::new(&mut self.context);

        event_object_initializer
            .property(
                js_string!(EVENT_VARIABLE_FIELD_NAME),
                js_string!(event.name.clone()),
                Attribute::READONLY,
            )
            .property(
                js_string!(EVENT_VARIABLE_FIELD_TYPE),
                js_string!(event.etype.name().to_string()),
                Attribute::READONLY,
            );
        event_object_initializer.property(
            js_string!(EVENT_VARIABLE_FIELD_SEND_ID),
            option_to_js_value(&event.sendid),
            Attribute::READONLY,
        );

        event_object_initializer.property(
            js_string!(EVENT_VARIABLE_FIELD_ORIGIN),
            option_to_js_value(&event.origin),
            Attribute::READONLY,
        );
        event_object_initializer.property(
            js_string!(EVENT_VARIABLE_FIELD_ORIGIN_TYPE),
            option_to_js_value(&event.origin_type),
            Attribute::READONLY,
        );
        event_object_initializer.property(
            js_string!(EVENT_VARIABLE_FIELD_INVOKE_ID),
            option_to_js_value(&event.invoke_id),
            Attribute::READONLY,
        );
        event_object_initializer.property(
            js_string!(EVENT_VARIABLE_FIELD_DATA),
            data_value,
            Attribute::READONLY,
        );

        let event_object = event_object_initializer.build();
        let r = self
            .context
            .global_object()
            .delete_property_or_throw(js_string!(EVENT_VARIABLE_NAME), &mut self.context);
        if let Err(error) = r {
            error!("Failed to delete old event: {}", error);
        }

        let r = self.context.global_object().define_property_or_throw(
            js_string!(EVENT_VARIABLE_NAME),
            PropertyDescriptor::builder()
                .configurable(true)
                .enumerable(false)
                .writable(false)
                .value(event_object),
            &mut self.context,
        );

        if let Err(error) = r {
            error!("Failed to set event: {}", error);
        }
    }

    fn assign(self: &mut ECMAScriptDatamodel, left_expr: &str, right_expr: &str) -> bool {
        self.assign_internal(left_expr, right_expr, false)
    }

    fn get_by_location(self: &mut ECMAScriptDatamodel, location: &str) -> Result<Data, String> {
        match self.execute_internal(location, false) {
            Err(msg) => {
                self.internal_error_execution();
                Err(msg)
            }
            Ok(val) => Ok(Data::new_moved(val)),
        }
    }

    fn get_io_processors(&mut self) -> &mut HashMap<String, Box<dyn EventIOProcessor>> {
        &mut self.io_processors
    }

    fn send(&mut self, ioc_processor: &str, target: &str, event: Event) -> bool {
        let ioc = self.io_processors.get_mut(ioc_processor);
        if let Some(ic) = ioc {
            ic.send(&self.global_data, target, event)
        } else {
            false
        }
    }

    fn get_mut(&mut self, name: &str) -> Option<&mut Data> {
        match self.data.get_mut(name) {
            Some(data) => Some(data),
            None => None,
        }
    }

    fn clear(self: &mut ECMAScriptDatamodel) {}

    fn log(&mut self, msg: &str) {
        info!("Log: {}", msg);
    }

    fn execute(&mut self, script: &str) -> Result<String, String> {
        self.execute_internal(script, true)
    }

    fn execute_for_each(
        &mut self,
        array_expression: &str,
        item_name: &str,
        index: &str,
        execute_body: &mut dyn FnMut(&mut dyn Datamodel) -> bool,
    ) -> bool {
        debug!("ForEach: array: {}", array_expression);
        match self.context.eval(Source::from_bytes(array_expression)) {
            Ok(r) => {
                match r.get_type() {
                    Type::Object => {
                        let obj = r.as_object().unwrap();
                        // Iterate through all members
                        let ob = obj.borrow();
                        let p = ob.properties();
                        let mut idx: i64 = 1;

                        if self.assign_internal(item_name, "null", true) {
                            for item_prop in p.index_property_values() {
                                // Skip the last "length" element
                                if item_prop.enumerable().is_some()
                                    && item_prop.enumerable().unwrap()
                                {
                                    match item_prop.value() {
                                        Some(item) => {
                                            debug!("ForEach: #{} {}={:?}", idx, item_name, item);
                                            let str = js_to_string(item, &mut self.context);
                                            if self.assign(item_name, str.as_str()) {
                                                if !index.is_empty() {
                                                    self.set_js_property(index, idx);
                                                }
                                                if !execute_body(self) {
                                                    return false;
                                                }
                                            } else {
                                                return false;
                                            }
                                        }
                                        None => {
                                            warn!("ForEach: #{} - failed to get value", idx,);
                                            return false;
                                        }
                                    }
                                    idx += 1;
                                }
                            }
                        }
                    }
                    _ => {
                        self.log("Resulting value is not a supported collection.");
                        self.internal_error_execution();
                    }
                }
                true
            }
            Err(e) => {
                self.log(&e.to_string());
                false
            }
        }
    }

    fn execute_condition(&mut self, script: &str) -> Result<bool, String> {
        // W3C:
        // B.2.3 Conditional Expressions
        //   The Processor must convert ECMAScript expressions used in conditional expressions into their effective boolean value using the ToBoolean operator
        //   as described in Section 9.2 of [ECMASCRIPT-262].
        let to_boolean_expression = format!("({})?true:false", script);
        match self.execute_internal(to_boolean_expression.as_str(), false) {
            Ok(val) => match bool::from_str(val.as_str()) {
                Ok(v) => Ok(v),
                Err(e) => Err(e.to_string()),
            },
            Err(msg) => Err(msg),
        }
    }

    #[allow(non_snake_case)]
    fn executeContent(&mut self, fsm: &Fsm, content_id: ExecutableContentId) -> bool {
        let ec = fsm.executableContent.get(&content_id);
        for e in ec.unwrap().iter() {
            if !self.execute_content(fsm, e.as_ref()) {
                return false;
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use log::info;
    use std::collections::HashMap;

    use crate::scxml_reader;
    use crate::test::run_test_manual;
    use crate::tracer::TraceMode;

    #[test]
    fn in_function() {
        info!("Creating The SM:");
        let sm = scxml_reader::parse_from_xml(
            r##"<scxml initial='Main' datamodel='ecmascript'>
              <state id='Main'>
                <onentry>
                   <if cond='In("Main")'>
                      <raise event='MainIsIn'/>
                   </if>
                </onentry>
                <transition event="MainIsIn" target="pass_1"/>
                <transition event="*" target="fail"/>
              </state>
              <state id='pass_1'>
                <onentry>
                   <if cond='In("Main")'>
                      <log expr='"Still in main?"'/>
                   <elseif cond='!In("Main")'/>
                      <raise event='MainIsNotIn'/>
                   </if>
                </onentry>
                <transition event="MainIsNotIn" target="pass"/>
                <transition event="*" target="fail"/>
              </state>
              <final id='pass'>
                <onentry>
                  <log label='Outcome' expr='"pass"'/>
                </onentry>
              </final>
              <final id="fail">
                <onentry>
                  <log label="Outcome" expr="'fail'"/>
                </onentry>
              </final>
            </scxml>"##
                .to_string(),
        );

        assert!(sm.is_ok(), "FSM shall be parsed");

        let fsm = sm.unwrap();
        let final_expected_configuration = vec!["pass".to_string()];

        assert!(run_test_manual(
            "In_function",
            &HashMap::new(),
            fsm,
            &Vec::new(),
            TraceMode::STATES,
            2000u64,
            &final_expected_configuration,
        ));
    }
}
