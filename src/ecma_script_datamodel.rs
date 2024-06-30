//! Implements the SCXML Data model for ECMA with Boa Engine.
//! See [W3C:The ECMAScript Data Model](https://www.w3.org/TR/scxml/#ecma-profile).
//! See [GitHub:Boa Engine](https://github.com/boa-dev/boa).

#[cfg(test)]
use std::{println as debug, println as info, println as warn, println as error};
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicU32, Ordering};

use boa_engine::{Context, js_string, JsError, JsValue, native_function::NativeFunction, Source};
use boa_engine::context::ContextBuilder;
use boa_engine::JsResult;
use boa_engine::object::builtins::{JsMap, JsSet};
use boa_engine::object::ObjectInitializer;
use boa_engine::property::Attribute;
use boa_engine::value::Type;
#[cfg(not(test))]
use log::{debug, error, info, warn};

use crate::access_global;
use crate::datamodel::{Data, Datamodel, DataStore, EVENT_VARIABLE_FIELD_DATA, EVENT_VARIABLE_FIELD_INVOKE_ID, EVENT_VARIABLE_FIELD_NAME, EVENT_VARIABLE_FIELD_ORIGIN, EVENT_VARIABLE_FIELD_ORIGIN_TYPE, EVENT_VARIABLE_FIELD_SEND_ID, EVENT_VARIABLE_FIELD_TYPE, EVENT_VARIABLE_NAME, GlobalDataAccess};
use crate::event_io_processor::{EventIOProcessor, SYS_IO_PROCESSORS};
use crate::executable_content::{DefaultExecutableContentTracer, ExecutableContent, ExecutableContentTracer};
use crate::fsm::{ExecutableContentId, Fsm, GlobalData, State, StateId};

pub const ECMA_SCRIPT: &str = "ECMAScript";
pub const ECMA_SCRIPT_LC: &str = "ecmascript";
pub const FSM_CONFIGURATION: &str = "_fsm_configuration";

static CONTEXT_ID_COUNTER: AtomicU32 = AtomicU32::new(1);


pub struct ECMAScriptDatamodel {
    pub data: DataStore,
    pub context_id: u32,
    pub global_data: GlobalDataAccess,
    pub context: Context,
    pub tracer: Option<Box<dyn ExecutableContentTracer>>,
    pub io_processors: HashMap<String, Box<dyn EventIOProcessor>>,
    pub id_to_state_names: HashMap<StateId, String>,
}

fn js_to_string(jv: &JsValue, ctx: &mut Context) -> String {
    match jv.to_string(ctx) {
        Ok(s) => {
            s.to_std_string().unwrap().clone()
        }
        Err(_e) => {
            jv.display().to_string()
        }
    }
}


fn log_js(_this: &JsValue, args: &[JsValue], ctx: &mut Context) -> Result<JsValue, JsError> {
    let mut msg = String::new();
    for arg in args {
        msg.push_str(js_to_string(arg, ctx).as_str());
    }
    info!("{}", msg);
    Ok(JsValue::Null)
}

impl ECMAScriptDatamodel {
    pub fn new() -> ECMAScriptDatamodel {
        let e = ECMAScriptDatamodel
        {
            data: DataStore::new(),
            context_id: CONTEXT_ID_COUNTER.fetch_add(1, Ordering::Relaxed),
            global_data: Arc::new(Mutex::new(GlobalData::new())),
            context: ContextBuilder::new().build().unwrap(),
            tracer: Some(Box::new(DefaultExecutableContentTracer::new())),
            io_processors: HashMap::new(),
            id_to_state_names: HashMap::new(),
        };
        e
    }

    fn execute_internal(&mut self, script: &str, handle_error: bool) -> Option<String> {
        let result = self.eval(script);
        match result {
            Ok(res) => {
                if res.is_undefined() {
                    debug!("Execute: {} => undefined", script );
                    None
                } else {
                    match res.to_string(&mut self.context) {
                        Ok(str) => {
                            let r = str.to_std_string_escaped();
                            debug!("Execute: {} => {}", script, r);
                            Some(r)
                        }
                        Err(err) => {
                            warn!("Script Error - failed to convert result to string: {} => {}", script, err);
                            if handle_error {
                                self.internal_error_execution();
                            }
                            None
                        }
                    }
                }
            }
            Err(e) => {
                // Pretty print the error
                error!("Script Error: {} => {} ", script, e.to_string());
                None
            }
        }
    }

    fn execute_content(&mut self, fsm: &Fsm, e: &dyn ExecutableContent) {
        match &mut self.tracer {
            Some(t) => {
                e.trace(t.as_mut(), fsm);
            }
            None => {}
        }
        e.execute(self, fsm);
    }

    fn eval(&mut self, source: &str) -> JsResult<JsValue> {
        self.update_global_data();
        self.context.eval(Source::from_bytes(source))
    }


    fn set_js_property<V>(&mut self, name: &str, value: V)
    where
        V: Into<JsValue>,
    {
        _ = self.context.global_object().set(js_string!(name), value, false, &mut self.context);
    }

    fn update_global_data(&mut self) {
        let set = JsSet::new(&mut self.context);
        for state in access_global!(self.global_data).configuration.iterator() {
            match self.id_to_state_names.get(state) {
                None => {
                    error!("State {} not found in state-names {:?}", state, self.id_to_state_names);
                }
                Some(state_name) => {
                    let _ = set.add(js_string!(state_name.as_str()), &mut self.context);
                }
            }
        }
        self.set_js_property(FSM_CONFIGURATION, set);
    }
}

impl Datamodel for ECMAScriptDatamodel {
    fn global(&mut self) -> &mut GlobalDataAccess {
        &mut self.global_data
    }
    fn global_s(&self) -> &GlobalDataAccess {
        &self.global_data
    }

    fn get_name(self: &Self) -> &str {
        return ECMA_SCRIPT;
    }

    fn implement_mandatory_functionality(&mut self, fsm: &mut Fsm) {
        let ctx = &mut self.context;

        // Implement "In" function.

        for state in fsm.states.as_slice() {
            self.id_to_state_names.insert(state.id, state.name.clone());
        }

        let _ = ctx.eval(
            Source::from_bytes(r##"
                function In(state) {
                   return _fsm_configuration.has( state );
                }
            "##)
        );

        // Implement "log" function.
        let _ = ctx.register_global_callable(js_string!("log"), 1, NativeFunction::from_copy_closure(log_js));

        // set system variable "_ioprocessors"
        {
            // Create I/O-Processor Objects.
            let io_processors_js = JsMap::new(ctx);
            for (name, processor) in &self.io_processors
            {
                let processor_js = JsMap::new(ctx);
                _ = processor_js.create_data_property(js_string!("location"), js_string!(processor.get_location()), ctx);
                // @TODO
                _ = io_processors_js.create_data_property(js_string!(name.as_str()), processor_js, ctx);
            }
            self.set_js_property(SYS_IO_PROCESSORS, io_processors_js);
        }
    }

    #[allow(non_snake_case)]
    fn initializeDataModel(&mut self, fsm: &mut Fsm, data_state: StateId) {
        let mut s = Vec::new();
        for (sn, _sid) in &fsm.statesNames {
            s.push(sn.clone());
        }
        let state_obj: &State = fsm.get_state_by_id_mut(data_state);

        // Set all (simple) global variables.
        for (name, data) in &state_obj.data.values
        {
            let rs = self.context.eval(Source::from_bytes(data.value.as_ref().unwrap().as_str()));
            match rs {
                Ok(val) => {
                    self.set_js_property(name.as_str(), val);
                }
                Err(_) => {
                    todo!()
                }
            }
        }
    }

    fn set(self: &mut ECMAScriptDatamodel, name: &str, data: Box<Data>) {
        let str_val = data.to_string().clone();
        self.data.set(name, data);
        self.set_js_property(name, js_string!(str_val));
    }

    fn set_event(&mut self, event: &crate::fsm::Event) {
        let mut data_object_initializer = ObjectInitializer::new(&mut self.context);

        for (key, value) in &event.data {
            data_object_initializer.property(js_string!(key.clone()), js_string!(value.clone()), Attribute::all());
        }
        let data_object = data_object_initializer.build();


        let event_object = ObjectInitializer::new(&mut self.context)
            .property(js_string!(EVENT_VARIABLE_FIELD_NAME), js_string!(event.name.clone()), Attribute::all())
            .property(js_string!(EVENT_VARIABLE_FIELD_TYPE), js_string!(event.etype.name().to_string()), Attribute::all())
            .property(js_string!(EVENT_VARIABLE_FIELD_SEND_ID), js_string!(event.sendid.clone()), Attribute::all())
            .property(js_string!(EVENT_VARIABLE_FIELD_ORIGIN), js_string!(event.origin.clone()), Attribute::all())
            .property(js_string!(EVENT_VARIABLE_FIELD_ORIGIN_TYPE), js_string!(event.origin_type.clone()), Attribute::all())
            .property(js_string!(EVENT_VARIABLE_FIELD_INVOKE_ID), event.invoke_id, Attribute::all())
            .property(js_string!(EVENT_VARIABLE_FIELD_DATA), data_object, Attribute::all())
            .build();

        _ = self.context.global_object().set(js_string!(EVENT_VARIABLE_NAME), event_object, false, &mut self.context);
    }

    fn assign(self: &mut ECMAScriptDatamodel, left_expr: &str, right_expr: &str) {
        let exp = format!("{}={}", left_expr, right_expr);
        let _ = self.eval(exp.as_str());
    }

    fn get_by_location(self: &mut ECMAScriptDatamodel, location: &str) -> Option<Data> {
        match self.execute_internal(location, false) {
            None => {
                self.internal_error_execution();
                None
            }
            Some(val) => {
                Some(Data::new_moved(val))
            }
        }
    }

    fn get_io_processors(&mut self) -> &mut HashMap<String, Box<dyn EventIOProcessor>> {
        return &mut self.io_processors;
    }

    fn get_mut<'v>(&'v mut self, name: &str) -> Option<&'v mut Data>
    {
        match self.data.get_mut(name) {
            Some(data) => {
                Some(data.as_mut())
            }
            None => {
                None
            }
        }
    }

    fn clear(self: &mut ECMAScriptDatamodel) {}

    fn log(&mut self, msg: &str) {
        info!("Log: {}", msg);
    }

    fn execute(&mut self, script: &str) -> Option<String> {
        self.execute_internal(script, true)
    }

    fn execute_for_each(&mut self, array_expression: &str, item_name: &str, index: &str,
                        execute_body: &mut dyn FnMut(&mut dyn Datamodel)) {
        debug!("ForEach: array: {}", array_expression );
        self.update_global_data();
        match self.context.eval(Source::from_bytes(array_expression)) {
            Ok(r) => {
                match r.get_type() {
                    Type::Object => {
                        let obj = r.as_object().unwrap();
                        // Iterate through all members
                        let ob = obj.borrow();
                        let p = ob.properties();
                        let mut idx: i64 = 1;
                        let _reg_item = self.set_js_property(item_name, JsValue::Null);
                        let item_declaration = self.context.eval(Source::from_bytes(item_name));
                        match item_declaration {
                            Ok(_) => {
                                for item_prop in p.index_property_values() {
                                    // Skip the last "length" element
                                    if item_prop.enumerable().is_some() && item_prop.enumerable().unwrap()
                                    {
                                        match item_prop.value() {
                                            Some(item) => {
                                                debug!("ForEach: #{} {}={:?}", idx , item_name, item );
                                                self.set_js_property(item_name, item.clone());
                                                if !index.is_empty() {
                                                    self.set_js_property(index, idx);
                                                }
                                                execute_body(self);
                                            }
                                            None => {
                                                warn!("ForEach: #{} - failed to get value", idx, );
                                            }
                                        }
                                        idx = idx + 1;
                                    }
                                }
                            }
                            Err(_) => {
                                self.log(format!("Item '{}' could not be declared.", item_name).as_str());
                                self.internal_error_execution();
                            }
                        }
                    }
                    _ => {
                        self.log(&"Resulting value is not a supported collection.".to_string());
                        self.internal_error_execution();
                    }
                }
            }
            Err(e) => {
                self.log(&e.to_string());
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
            Some(val) => {
                match bool::from_str(val.as_str()) {
                    Ok(v) => Ok(v),
                    Err(e) => Err(e.to_string()),
                }
            }
            None => Err("undefined".to_string())
        }
    }

    #[allow(non_snake_case)]
    fn executeContent(&mut self, fsm: &Fsm, content_id: ExecutableContentId) {
        for (_idx, e) in fsm.executableContent.get(&content_id).unwrap().iter().enumerate() {
            self.execute_content(fsm, e.as_ref());
        }
    }
}

#[cfg(test)]
mod tests {
    use log::info;

    use crate::reader;
    use crate::test::run_test_manual;
    use crate::tracer::TraceMode;

    #[test]
    fn in_function() {
        info!("Creating The SM:");
        let sm = reader::read_from_xml(
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
            </scxml>"##.to_string());

        assert!(!sm.is_err(), "FSM shall be parsed");

        let fsm = sm.unwrap();
        let mut final_expected_configuration = Vec::new();
        final_expected_configuration.push("pass".to_string());

        assert!(run_test_manual(&"In_function", fsm, TraceMode::STATES, 2000 as u64, &final_expected_configuration));
    }
}

