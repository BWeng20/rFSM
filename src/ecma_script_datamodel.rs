//! Implements the SCXML Data model for ECMA with Boa Engine.
//! See [W3C:The ECMAScript Data Model](https://www.w3.org/TR/scxml/#ecma-profile).
//! See [GitHub:Boa Engine](https://github.com/boa-dev/boa).

use std::collections::HashMap;
use std::str::FromStr;
use std::sync::atomic::{AtomicU32, Ordering};

use boa_engine::{
    Context,
    js_string,
    JsError,
    JsNativeError,
    JsValue, native_function::NativeFunction, property::Attribute, Source,
};
use boa_engine::JsResult;
use boa_engine::object::builtins::JsMap;
use boa_engine::value::Type;
use boa_gc::GcRefCell;
use log::{debug, error, info, warn};

use crate::datamodel::{Data, Datamodel, DataStore};
use crate::event_io_processor::{EventIOProcessor, SYS_IO_PROCESSORS};
use crate::executable_content::{DefaultExecutableContentTracer, ExecutableContent, ExecutableContentTracer};
use crate::fsm::{ExecutableContentId, Fsm, GlobalData, State, StateId};

pub const ECMA_SCRIPT: &str = "ECMAScript";
pub const ECMA_SCRIPT_LC: &str = "ecmascript";


static CONTEXT_ID_COUNTER: AtomicU32 = AtomicU32::new(1);


pub struct ECMAScriptDatamodel {
    pub data: DataStore,
    pub context_id: u32,
    pub global_data: GlobalData,
    pub context: Context,
    pub tracer: Option<Box<dyn ExecutableContentTracer>>,
    pub io_processors: HashMap<String, Box<dyn EventIOProcessor>>,
    pub all_names: Vec<String>,
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
            global_data: GlobalData::new(),
            context: Context::default(),
            tracer: Some(Box::new(DefaultExecutableContentTracer::new())),
            io_processors: HashMap::new(),
            all_names: Vec::new(),
        };
        e
    }

    fn execute_internal(&mut self, _fsm: &Fsm, script: &str) -> String {
        let mut r: String = "".to_string();

        let result = self.eval(script);
        match result {
            Ok(res) => {
                match res.to_string(&mut self.context) {
                    Ok(str) => {
                        r = str.to_std_string_escaped();
                        debug!("Execute: {} => {}", script, r);
                    }
                    Err(err) => {
                        warn!("Script Error - failed to convert result to string: {} => {}", script, err);
                    }
                }
            }
            Err(e) => {
                // Pretty print the error
                error!("Script Error: {} => {} ", script, e.to_string());
            }
        }

        r
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
        self.context.eval(Source::from_bytes(source))
    }
}


impl Datamodel for ECMAScriptDatamodel {
    fn global(&mut self) -> &mut GlobalData {
        &mut self.global_data
    }
    fn global_s(&self) -> &GlobalData {
        &self.global_data
    }

    fn get_name(self: &Self) -> &str {
        return ECMA_SCRIPT;
    }

    fn implement_mandatory_functionality(&mut self, fsm: &mut Fsm) {

        // Implement "In" function.
        for (sn, _sid) in &fsm.statesNames {
            _ = self.all_names.push(sn.clone());
        }

        let ctx = &mut self.context;

        let _ = ctx.register_global_callable(js_string!("In"), 1,
                                             NativeFunction::from_copy_closure_with_captures(move |_this: &JsValue, args: &[JsValue], captures, ctx: &mut Context| {
                                                 let mut captures = captures.borrow_mut();
                                                 let all_names = &mut *captures;
                                                 if args.len() > 0 {
                                                     let name = &js_to_string(&args[0], ctx);

                                                     let m = all_names.contains(name);
                                                     Ok(JsValue::from(m))
                                                 } else {
                                                     Err(JsNativeError::typ().with_message("Missing argument").into())
                                                 }
                                             }, GcRefCell::new(self.all_names.clone())));

        let _ = ctx.register_global_callable(js_string!("log"), 1,
                                             NativeFunction::from_copy_closure(log_js));

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
            _ = self.context.register_global_property(js_string!(SYS_IO_PROCESSORS), io_processors_js, Attribute::all());
        }
    }

    #[allow(non_snake_case)]
    fn initializeDataModel(&mut self, fsm: &mut Fsm, data_state: StateId) {
        let mut s = Vec::new();
        for (sn, _sid) in &fsm.statesNames {
            s.push(sn.clone());
        }
        let state_obj: &State = fsm.get_state_by_id_mut(data_state);
        let ctx = &mut self.context;

        // Set all (simple) global variables.
        for (name, data) in &state_obj.data.values
        {
            match ctx.eval(Source::from_bytes(data.value.as_ref().unwrap().as_str())) {
                Ok(val) => {
                    _ = ctx.register_global_property(js_string!(name.as_str()), val, Attribute::all());
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
        _ = self.context.register_global_property(js_string!(name), js_string!(str_val), Attribute::all());
    }

    fn set_event(&mut self, _event: &crate::fsm::Event) {
        // TODO
    }

    fn assign(self: &mut ECMAScriptDatamodel, _fsm: &Fsm, left_expr: &str, right_expr: &str) {
        let exp = format!("{}={}", left_expr, right_expr);
        let _ = self.eval(exp.as_str());
    }

    fn get(self: &ECMAScriptDatamodel, name: &str) -> Option<&Data> {
        match self.data.get(name) {
            Some(data) => {
                Some(&**data)
            }
            None => {
                None
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

    fn execute(&mut self, fsm: &Fsm, script: &str) -> String {
        self.execute_internal(fsm, script)
    }

    fn execute_for_each(&mut self, _fsm: &Fsm, array_expression: &str, item_name: &str, index: &str,
                        execute_body: &mut dyn FnMut(&mut dyn Datamodel)) {
        debug!("ForEach: array: {}", array_expression );
        match self.context.eval(Source::from_bytes(array_expression)) {
            Ok(r) => {
                match r.get_type() {
                    Type::Object => {
                        let obj = r.as_object().unwrap();
                        // Iterate through all members
                        let ob = obj.borrow();
                        let p = ob.properties();
                        let mut idx: i64 = 1;
                        let _reg_item = self.context.register_global_property(js_string!(item_name), JsValue::Null, Attribute::all());
                        let item_declaration = self.eval(item_name);
                        match item_declaration {
                            Ok(_) => {
                                for item_prop in p.index_property_values() {
                                    // Skip the last "length" element
                                    if item_prop.enumerable().is_some() && item_prop.enumerable().unwrap()
                                    {
                                        match item_prop.value() {
                                            Some(item) => {
                                                debug!("ForEach: #{} {}={:?}", idx , item_name, item );
                                                let _ = self.context.register_global_property(js_string!(item_name), item.clone(), Attribute::all());
                                                if !index.is_empty() {
                                                    let _ = self.context.register_global_property(js_string!(index), idx, Attribute::all());
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


    fn execute_condition(&mut self, fsm: &Fsm, script: &str) -> Result<bool, String> {
        // W3C:
        // B.2.3 Conditional Expressions
        //   The Processor must convert ECMAScript expressions used in conditional expressions into their effective boolean value using the ToBoolean operator
        //   as described in Section 9.2 of [ECMASCRIPT-262].
        let to_boolean_expression = format!("({})?true:false", script);
        let r = self.execute_internal(fsm, to_boolean_expression.as_str());
        match bool::from_str(r.as_str()) {
            Ok(v) => Ok(v),
            Err(e) => Err(e.to_string()),
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
    use std::{time};
    use crate::reader;
    use crate::test::run_test_manual;
    use crate::tracer::TraceMode;

    #[test]
    fn In_function() {
        println!("Creating The SM:");
        let sm = reader::read_from_xml(
            r##"<scxml initial='Main' datamodel='ecmascript'>
              <state id='Main'>
                <onentry>
                   <if cond='In(\'Main\')'>
                      <raise event='MainIsIn'/>
                   </if>
                </onentry>
                <transition event="MainIsIn" target="pass"/>
                <transition event="*" target="fail"/>
              </state>
              <final id='Pass'>
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

        let mut fsm = sm.unwrap();
        let mut final_expected_configuration = Vec::new();
        final_expected_configuration.push("main".to_string());

        assert!( !run_test_manual(&"In_function", fsm, TraceMode::STATES, 2000 as u64, &final_expected_configuration ) );
    }
}

