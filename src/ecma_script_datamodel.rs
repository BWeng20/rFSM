//! Implements the SCXML Data model for ECMA with Boa Engine.
//! See [W3C:The ECMAScript Data Model](https://www.w3.org/TR/scxml/#ecma-profile).
//! See [Github:Boa Engine](https://github.com/boa-dev/boa).

use std::collections::{HashMap, LinkedList};
use std::str::FromStr;
use std::sync::atomic::{AtomicU32, Ordering};

use boa_engine::{Context, JsResult, JsString, JsValue, property::Attribute};
use boa_engine::object::{FunctionBuilder, JsMap};
use boa_engine::value::Type;
use log::{debug, error, info, warn};

use crate::datamodel::{Data, Datamodel, DataStore, StringData};
use crate::event_io_processor::{EventIOProcessor, SYS_IO_PROCESSORS};
use crate::executable_content::{DefaultExecutableContentTracer, ExecutableContentTracer};
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
    pub stack: LinkedList<(String, Option<Box<dyn Data>>)>,
}

fn js_to_string(jv: &JsValue, ctx: &mut Context) -> String {
    match jv.to_string(ctx) {
        Ok(s) => {
            s.to_string()
        }
        Err(_e) => {
            jv.display().to_string()
        }
    }
}


fn log_js(_this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let mut msg = String::new();
    for arg in args {
        msg.push_str(js_to_string(arg, ctx).as_str());
    }
    info!("{}", msg);
    Ok(JsValue::from(msg))
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
            stack: LinkedList::new(),
        };
        e
    }


    fn execute_internal(&mut self, _fsm: &Fsm, script: &str) -> String {
        let mut r: String = "".to_string();

        let stack_pos = self.stack.len();
        self.update_data();

        let result = self.context.eval(script);
        match result {
            Ok(res) => {
                r = res.to_string(&mut self.context).unwrap().to_string();
                debug!("Execute: {} => {}", script, r);
            }
            Err(e) => {
                // Pretty print the error
                error!("Script Error {}", e.display());
            }
        }
        while stack_pos > self.stack.len()
        {
            self.pop_property();
        }

        r
    }

    fn update_data(&mut self) {
        // Set all (simple) global variables.
        let mut ll = HashMap::new();
        for (name, data) in &self.data.values
        {
            // @TODO: push only changed data
            ll.insert(name.clone(), data.get_copy());
        }

        let it = ll.into_iter();
        it.for_each(move |(name, data)| {
            self.push_data(name.as_str(), data);
        });
    }

    /// Sets a data-property, saving the current state  on stack.
    /// Property will be restored with the matching "pop_property"
    /// If the property is not available he next "pop_property" will remove it.
    fn push_data(&mut self, name: &str, data: Box<dyn Data>)
    {
        match self.data.values.remove(name) {
            Some(val) => {
                self.stack.push_back((name.to_string(), Some(val)));
            }
            None => {
                self.stack.push_back((name.to_string(), None));
            }
        }

        let js: JsValue;
        if data.is_numeric() {
            js = JsValue::Rational(data.as_number());
        } else {
            js = JsValue::String(JsString::new(data.to_string()));
        }
        // @TODO: handle also complex data.
        self.context.register_global_property(name, js, Attribute::all());
        self.data.values.insert(name.to_string(), data);
    }

    /// Sets a property, saving the current state  on stack.
    /// Property will be restored with the matching "pop_property".
    /// If the property is not available he next "pop_property" will remove it.
    fn push_property(&mut self, name: &str, data: &str)
    {
        match self.data.values.remove(name) {
            Some(val) => {
                self.stack.push_back((name.to_string(), Some(val)));
            }
            None => {
                self.stack.push_back((name.to_string(), None));
            }
        }
        self.data.values.insert(name.to_string(), Box::new(StringData::new(data)));
        self.context.register_global_property(name, JsString::new(data), Attribute::all());
    }

    /// Pops the last property from stack, restoring it.
    fn pop_property(&mut self)
    {
        match self.stack.pop_back() {
            Some((name, value)) => {
                match value {
                    None => {
                        self.data.values.remove(name.as_str());
                    }
                    Some(val) => {
                        self.data.values.insert(name, val);
                    }
                }
            }
            None => {}
        }
    }
}

/**
 * ECMAScript data model
 */
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

    #[allow(non_snake_case)]
    fn initializeDataModel(&mut self, fsm: &mut Fsm, data_state: StateId) {
        let mut s = Vec::new();
        for (sn, _sid) in &fsm.statesNames {
            s.push(sn.clone());
        }

        let state_obj: &mut State = fsm.get_state_by_id_mut(data_state);

        let ctx = &mut self.context;

        ctx.register_global_builtin_function("log", 1, log_js);

        // Implement "In" function.
        FunctionBuilder::closure_with_captures(ctx,
                                               move |_this: &JsValue, args: &[JsValue], names: &mut Vec<String>, ctx: &mut Context| {
                                                   if args.len() > 0 {
                                                       let name = &js_to_string(&args[0], ctx);
                                                       let m = names.contains(name);
                                                       Ok(JsValue::from(m))
                                                   } else {
                                                       Err(JsValue::from("Missing argument"))
                                                   }
                                               }, s).name("In").length(1).build();

        // Set all (simple) global variables.
        for (name, data) in &state_obj.data.values
        {
            self.data.values.insert(name.clone(), data.get_copy());
            ctx.register_global_property(name.as_str(), data.to_string(), Attribute::all());
        }

        // set system variable "_ioprocessors"
        {
            // Create I/O-Processor Objects.
            let io_processors_js = JsMap::new(ctx);
            for (name, processor) in &self.io_processors
            {
                let processor_js = JsMap::new(ctx);
                _ = processor_js.create_data_property("location", processor.get_location(), ctx);
                // @TODO
                _ = io_processors_js.create_data_property(name.as_str(), processor_js, ctx);
            }
            self.context.register_global_property(SYS_IO_PROCESSORS, io_processors_js, Attribute::all());
        }
    }

    fn set(self: &mut ECMAScriptDatamodel, name: &str, data: Box<dyn Data>) {
        let str_val = data.to_string().clone();
        self.data.set(name, data);
        // TODO: Set data also in the Context
        self.context.register_global_property(name, JsString::new(str_val), Attribute::all());
    }

    fn get(self: &ECMAScriptDatamodel, name: &str) -> Option<&dyn Data> {
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

    fn get_mut<'v>(&'v mut self, name: &str) -> Option<&'v mut dyn Data>
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
        match self.context.eval(array_expression) {
            Ok(r) => {
                match r.get_type() {
                    Type::Object => {
                        let obj = r.as_object().unwrap();
                        // Iterate through all members
                        let ob = obj.borrow();
                        let p = ob.properties();
                        let mut idx: i64 = 0;
                        for item_prop in p.values() {
                            match item_prop.value() {
                                Some(item) => {
                                    let str_val = js_to_string(&item, &mut self.context);
                                    debug!("ForEach: #{} {}", idx, str_val.as_str() );
                                    self.push_property(item_name, str_val.as_str());
                                    self.context.register_global_property(item_name, item, Attribute::all());
                                    if !index.is_empty() {
                                        self.context.register_global_property(index, idx, Attribute::all());
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
                    _ => {
                        self.log(&"Resulting value is not a supported collection.".to_string());
                    }
                }
            }
            Err(e) => {
                self.log(&e.display().to_string());
            }
        }
    }


    fn execute_condition(&mut self, fsm: &Fsm, script: &str) -> Result<bool, String> {
        match bool::from_str(self.execute_internal(fsm, script).as_str()) {
            Ok(v) => Ok(v),
            Err(e) => Err(e.to_string()),
        }
    }

    #[allow(non_snake_case)]
    fn executeContent(&mut self, fsm: &Fsm, content_id: ExecutableContentId) {
        for (_idx, e) in fsm.executableContent.get(&content_id).unwrap().iter().enumerate() {
            match &mut self.tracer {
                Some(t) => {
                    e.trace(t.as_mut(), fsm);
                }
                None => {}
            }
            e.execute(self, fsm);
        }
    }
}



