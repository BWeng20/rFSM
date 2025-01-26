//! Implements the SCXML Data model for ECMA with Boa Engine.\
//! Included if feature "ECMAScriptModel" is enabled.\
//! See [W3C:The ECMAScript Data Model](/doc/W3C_SCXML_2024_07_13/index.html#ecma-profile).\
//! See [GitHub:Boa Engine](https://github.com/boa-dev/boa).

use crate::common::ArgOption;
use crate::common::{debug, error, info, warn};
use boa_engine::context::ContextBuilder;
use boa_engine::object::builtins::{JsArray, JsMap};
use boa_engine::object::ObjectInitializer;
use boa_engine::property::{Attribute, PropertyDescriptor, PropertyKey};
use boa_engine::value::Type;
use boa_engine::{js_string, native_function::NativeFunction, Context, JsBigInt, JsError, JsValue, Source};
use boa_engine::{JsArgs, JsData, JsResult};
use boa_gc::{empty_trace, Finalize, Trace};
use std::collections::HashMap;
use std::ops::Deref;
use std::string::ToString;

use crate::datamodel::{
    create_data_arc, str_to_source, Data, DataArc, Datamodel, DatamodelFactory, GlobalDataArc,
    EVENT_VARIABLE_FIELD_DATA, EVENT_VARIABLE_FIELD_INVOKE_ID, EVENT_VARIABLE_FIELD_NAME, EVENT_VARIABLE_FIELD_ORIGIN,
    EVENT_VARIABLE_FIELD_ORIGIN_TYPE, EVENT_VARIABLE_FIELD_SEND_ID, EVENT_VARIABLE_FIELD_TYPE, EVENT_VARIABLE_NAME,
};
use crate::event_io_processor::SYS_IO_PROCESSORS;

#[cfg(feature = "Trace")]
use crate::executable_content::DefaultExecutableContentTracer;

use crate::executable_content::{ExecutableContent, ExecutableContentTracer};
use crate::fsm::{ExecutableContentId, Fsm, StateId};

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
    pub global_data: GlobalDataArc,
    pub context: Context,
    pub tracer: Option<Box<dyn ExecutableContentTracer>>,
    pub strict_mode: bool,
}

pub struct ECMAScriptDatamodelFactory {}

impl DatamodelFactory for ECMAScriptDatamodelFactory {
    fn create(&mut self, global_data: GlobalDataArc, options: &HashMap<String, String>) -> Box<dyn Datamodel> {
        let mut ecma = Box::new(ECMAScriptDatamodel::new(global_data));
        for (key, value) in options {
            ecma.set_option(key.as_str(), value.as_str());
        }
        ecma
    }
}

fn js_to_string(jv: &JsValue, ctx: &mut Context) -> String {
    match jv.to_string(ctx) {
        Ok(s) => s.to_std_string().unwrap().clone(),
        Err(_e) => jv.display().to_string(),
    }
}

fn data_value_to_js(data: &Data, context: &mut Context) -> JsValue {
    match data {
        Data::None() => JsValue::Undefined,
        Data::Null() => JsValue::Null,
        Data::Integer(v) => JsValue::BigInt(JsBigInt::from(*v)),
        Data::Double(v) => JsValue::Rational(*v),
        Data::Boolean(v) => JsValue::Boolean(*v),
        Data::String(v) => JsValue::String(js_string!(v.clone())),
        Data::Array(v) => {
            let js_array = JsArray::new(context);
            for data in v {
                let djs = match data.lock() {
                    Ok(l) => data_value_to_js(l.deref(), context),
                    Err(_) => JsValue::Null,
                };
                let _ = js_array.push(djs, context);
            }
            JsValue::from(js_array)
        }
        Data::Map(v) => {
            let js_map = JsMap::new(context);
            for (key, d) in v {
                let djs = data_value_to_js(&d.lock().unwrap(), context);
                let _ = js_map.set(js_string!(key.clone()), djs, context);
            }
            JsValue::from(js_map)
        }
        Data::Error(_error) => JsValue::Null,
        Data::Source(source) => JsValue::String(js_string!(source.source.clone())),
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
    pub global_data: GlobalDataArc,
    pub state_name_to_id: HashMap<String, StateId>,
}

/// Dummy implementation for the Wrapper to enable us to add this class to the context.\
/// Safety: Nothing in this struct needs tracing, so this is safe.
unsafe impl Trace for FsmJSWrapper {
    empty_trace!();
}

impl FsmJSWrapper {
    fn new(gd: GlobalDataArc) -> FsmJSWrapper {
        FsmJSWrapper {
            global_data: gd,
            state_name_to_id: HashMap::new(),
        }
    }
}

impl ECMAScriptDatamodel {
    pub fn new(global_data: GlobalDataArc) -> ECMAScriptDatamodel {
        ECMAScriptDatamodel {
            global_data,
            context: ContextBuilder::new().build().unwrap(),
            #[cfg(feature = "Trace")]
            tracer: Some(Box::new(DefaultExecutableContentTracer::new())),
            #[cfg(not(feature = "Trace"))]
            tracer: None,
            strict_mode: false,
        }
    }

    pub fn js_to_data_value(value: &JsValue, ctx: &mut Context) -> Result<Data, String> {
        #[cfg(feature = "Debug")]
        debug!("js2d {:?} -> {:?}", value, value.get_type());
        match value.get_type() {
            Type::Undefined => Ok(Data::None()),
            Type::Null => Ok(Data::Null()),
            Type::Boolean => Ok(Data::Boolean(value.as_boolean().unwrap())),
            Type::Number => Ok(Data::Double(value.as_number().unwrap())),
            Type::Symbol | Type::String => Ok(Data::String(js_to_string(value, ctx))),
            Type::BigInt => match value.to_big_int64(ctx) {
                Ok(val) => Ok(Data::Integer(val)),
                Err(err) => {
                    error!("Can't converted '{:?}' to Data::Integer: {}", value, err);
                    Ok(Data::None())
                }
            },
            Type::Object => match value.to_object(ctx) {
                Ok(obj) => {
                    if obj.is_array() {
                        let ar = JsArray::from_object(obj.clone()).unwrap();
                        let len = ar.length(ctx).unwrap() as usize;
                        #[cfg(feature = "Debug")]
                        debug!("js2d array #{} >>", len);
                        let mut dv = Vec::with_capacity(len);
                        for i in 0..len {
                            let v = ar.get(i, ctx).unwrap();
                            if let Ok(av) = Self::js_to_data_value(&v, ctx) {
                                dv.push(create_data_arc(av))
                            }
                        }
                        #[cfg(feature = "Debug")]
                        debug!("<< js2d array #{}", dv.len());
                        Ok(Data::Array(dv))
                    } else {
                        let keys = obj.own_property_keys(ctx).unwrap();
                        #[cfg(feature = "Debug")]
                        debug!("js2d object #{} >>", keys.len());
                        let mut dvm = HashMap::with_capacity(keys.len());
                        for key in &keys {
                            let name = match key {
                                PropertyKey::String(ref name) => name.to_std_string().unwrap(),
                                PropertyKey::Symbol(ref name) => name.fn_name().to_std_string().unwrap(),
                                PropertyKey::Index(idx) => idx.get().to_string(),
                            };
                            #[cfg(feature = "Debug")]
                            debug!("key '{}'", key);
                            let js = obj.get(key.clone(), ctx).unwrap();
                            match Self::js_to_data_value(&js, ctx) {
                                Err(err) => {
                                    warn!("{}", err)
                                }
                                Ok(dt) => {
                                    dvm.insert(name, create_data_arc(dt));
                                }
                            }
                        }
                        #[cfg(feature = "Debug")]
                        debug!("<< js2d object #{}", dvm.len());
                        Ok(Data::Map(dvm))
                    }
                }
                Err(err) => Err(format!("Can't converted '{:?}' to Data: {}", value, err)),
            },
        }
    }

    pub fn set_option(&mut self, name: &str, _value: &str) {
        if let Some(ecma_option) = name.strip_prefix(ECMA_OPTION_INFIX) {
            match ecma_option {
                ECMA_OPTION_STRICT_POSTFIX => {
                    #[cfg(feature = "Debug")]
                    debug!("Running ECMA in strict mode");
                    self.strict_mode = true;
                    self.context.strict(true);
                }
                &_ => {}
            }
        }
    }

    fn execute_internal(&mut self, script: &Data, handle_error: bool) -> Result<DataArc, String> {
        let result = self.eval(script);
        match result {
            Ok(res) => {
                if res.is_undefined() {
                    #[cfg(feature = "Debug")]
                    debug!("Execute: {} => undefined", script);
                    Ok(create_data_arc(Data::Null()))
                } else {
                    #[cfg(feature = "Debug")]
                    debug!("Execute: {} => {:?}", script, res);
                    match Self::js_to_data_value(&res, &mut self.context) {
                        Ok(s) => Ok(create_data_arc(s)),
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

    fn eval(&mut self, source: &Data) -> JsResult<JsValue> {
        self.context.eval(Source::from_bytes(&source.as_script()))
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

    fn assign_internal(&mut self, left_expr: &str, right_expr: &str, allow_undefined: bool) -> bool {
        let exp = format!("{}={}", left_expr, right_expr);
        if allow_undefined && self.strict_mode {
            self.context.strict(false);
        }
        let r = match self.eval(&str_to_source(exp.as_str())) {
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

    pub fn data_arc_to_js(&mut self, data: &DataArc) -> JsValue {
        match data.lock() {
            Ok(l) => self.data_value_to_js(l.deref()),
            Err(_) => JsValue::Null,
        }
    }

    pub fn data_value_to_js(&mut self, data: &Data) -> JsValue {
        data_value_to_js(data, &mut self.context)
    }

    fn call_action(_this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
        let mut arg_list = Vec::<Data>::with_capacity(0);
        let action_name = js_to_string(args.get_or_undefined(0), ctx);
        {
            let arguments = args.get_or_undefined(1);

            if let Some(obj) = arguments.as_object() {
                if obj.is_array() {
                    let ar = JsArray::from_object(obj.clone()).unwrap();
                    let len = ar.length(ctx).unwrap() as usize;
                    arg_list.reserve(len);
                    for i in 0..len {
                        let v = ar.get(i, ctx).unwrap();
                        if let Ok(av) = Self::js_to_data_value(&v, ctx) {
                            arg_list.push(av)
                        }
                    }
                } else {
                    error!("Arguments must be specified as Array: {:?}", obj);
                }
            } else {
                error!("Arguments must be specified as Array: {:?}", arguments);
            }
        }
        let r = if let Some(fsm) = ctx.get_data::<FsmJSWrapper>() {
            let global_data = fsm.global_data.lock().unwrap();
            global_data
                .actions
                .execute(action_name.as_str(), &arg_list, &global_data)
        } else {
            Err("Failed".to_string())
        };
        match r {
            Ok(v) => Ok(data_value_to_js(&v, ctx)),
            Err(v) => Err(JsError::from_opaque(JsValue::from(js_string!(v)))),
        }
    }

    fn in_configuration(_this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
        let state = args.get_or_undefined(0);

        if let Ok(name) = state.to_string(context) {
            let fsm = context.get_data::<FsmJSWrapper>().unwrap();
            let loc = fsm.state_name_to_id.get(&name.to_std_string().unwrap());
            match loc {
                None => {}
                Some(state_id) => {
                    if fsm
                        .global_data
                        .lock()
                        .unwrap()
                        .configuration
                        .data
                        .contains(state_id)
                    {
                        return Ok(JsValue::Boolean(true));
                    }
                }
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
    fn global(&mut self) -> &mut GlobalDataArc {
        &mut self.global_data
    }
    fn global_s(&self) -> &GlobalDataArc {
        &self.global_data
    }

    fn get_name(&self) -> &str {
        ECMA_SCRIPT
    }

    fn add_functions(&mut self, fsm: &mut Fsm) {
        let mut functions = String::new();
        for name in self.global_s().lock().unwrap().actions.lock().keys() {
            functions.push_str(
                format!(
                    "function {}(){{ return __action('{}', Array.from(arguments)); }}\n",
                    name, name
                )
                .as_str(),
            );
        }

        let ctx = &mut self.context;

        // Implement "action" function.
        let _ = ctx.register_global_callable(
            js_string!("__action"),
            2,
            NativeFunction::from_copy_closure(Self::call_action),
        );

        let r = ctx.eval(Source::from_bytes(functions.as_str()));
        if let Err(err) = r {
            error!("Failed to add actions: {}", err);
        }

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
    }

    /// set system variable "_ioprocessors"
    fn set_ioprocessors(&mut self) {
        {
            let session_id = self.global_s().lock().unwrap().session_id;
            let ctx = &mut self.context;

            // Create I/O-Processor Objects.
            let io_processors_js = JsMap::new(ctx);
            for (name, processor) in &self.global_data.lock().unwrap().io_processors {
                let processor_js = JsMap::new(ctx);
                let location = js_string!(processor.lock().unwrap().get_location(session_id));
                _ = processor_js.create_data_property(js_string!("location"), location, ctx);
                _ = io_processors_js.create_data_property(js_string!(name.as_str()), processor_js, ctx);
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

    fn set_from_state_data(&mut self, data: &HashMap<String, DataArc>, set_data: bool) {
        for (name, data) in data {
            if set_data {
                let data_guard = data.lock().unwrap();
                if let Data::Source(src) = data_guard.deref() {
                    if !src.is_empty() {
                        let rs = self.context.eval(Source::from_bytes(src.as_str()));
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
                    } else {
                        self.set_js_property(name.as_str(), JsValue::Null);
                    };
                } else {
                    let ds = self.data_value_to_js(data_guard.deref());
                    self.set_js_property(name.as_str(), ds);
                }
            } else {
                self.set_js_property(name.as_str(), JsValue::Undefined);
            }
        }
    }

    fn initialize_read_only_arc(&mut self, name: &str, value: DataArc) {
        let r = self.context.global_object().define_property_or_throw(
            js_string!(name),
            PropertyDescriptor::builder()
                .configurable(true)
                .enumerable(false)
                .writable(false)
                .value(self.data_arc_to_js(&value)),
            &mut self.context,
        );
        if let Err(error) = r {
            error!("Failed to initialize read only {}: {}", name, error);
        }
    }

    fn set_arc(&mut self, name: &str, data: DataArc, allow_undefined: bool) {
        let v = self.data_arc_to_js(&data);
        self.set_js_property(name, v);
        if allow_undefined {
            self.global_data
                .lock()
                .unwrap()
                .data
                .set_undefined_arc(name.to_string(), data);
        } else {
            self.global_data
                .lock()
                .unwrap()
                .data
                .set_arc(name.to_string(), data);
        }
    }

    fn set_event(&mut self, event: &crate::fsm::Event) {
        let data_value = match &event.param_values {
            None => match &event.content {
                None => JsValue::Undefined,
                Some(c) => self.data_value_to_js(c),
            },
            Some(pv) => {
                let mut data = Vec::with_capacity(pv.len());

                for pair in pv.iter() {
                    data.push((
                        js_string!(pair.name.clone()),
                        self.data_arc_to_js(&create_data_arc(pair.value.clone())),
                    ));
                }
                let ctx = &mut self.context;
                let mut data_object_initializer = ObjectInitializer::new(ctx);
                for (dn, dv) in data {
                    data_object_initializer.property(dn, dv, Attribute::all());
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
            error!("Failed to delete exiting event: {}", error);
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

    fn assign(self: &mut ECMAScriptDatamodel, left_expr: &Data, right_expr: &Data) -> bool {
        self.assign_internal(
            left_expr.as_script().as_str(),
            right_expr.as_script().as_str(),
            false,
        )
    }

    fn get_by_location(self: &mut ECMAScriptDatamodel, location: &str) -> Result<DataArc, String> {
        match self.execute_internal(&str_to_source(location), false) {
            Err(msg) => {
                self.internal_error_execution();
                Err(msg)
            }
            Ok(val) => Ok(val),
        }
    }

    fn clear(self: &mut ECMAScriptDatamodel) {}

    fn execute(&mut self, script: &Data) -> Result<DataArc, String> {
        self.execute_internal(script, true)
    }

    fn execute_for_each(
        &mut self,
        array_expression: &Data,
        item_name: &str,
        index: &str,
        execute_body: &mut dyn FnMut(&mut dyn Datamodel) -> bool,
    ) -> bool {
        #[cfg(feature = "Debug")]
        debug!("ForEach: array: {}", array_expression);
        match self
            .context
            .eval(Source::from_bytes(&array_expression.as_script()))
        {
            Ok(r) => {
                match r.get_type() {
                    Type::Object => {
                        let obj = r.as_object().unwrap();
                        // Iterate through all members
                        let ob = obj.borrow();
                        let p = ob.properties();
                        let mut idx: i64 = 0;

                        if self.assign_internal(item_name, "null", true) {
                            for item_prop in p.index_property_values() {
                                // Skip the last "length" element
                                if item_prop.enumerable().is_some() && item_prop.enumerable().unwrap() {
                                    match item_prop.value() {
                                        Some(item) => {
                                            #[cfg(feature = "Debug")]
                                            debug!("ForEach: #{} {}={:?}", idx, item_name, item);
                                            let str = js_to_string(item, &mut self.context);
                                            if self.assign(&str_to_source(item_name), &str_to_source(str.as_str())) {
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

    fn execute_condition(&mut self, script: &Data) -> Result<bool, String> {
        // W3C:
        // B.2.3 Conditional Expressions
        //   The Processor must convert ECMAScript expressions used in conditional expressions into their effective boolean value using the ToBoolean operator
        //   as described in Section 9.2 of [ECMASCRIPT-262].
        let to_boolean_expression = format!("({})?true:false", script.as_script());
        match self.execute_internal(&str_to_source(to_boolean_expression.as_str()), false) {
            Ok(val) => match val.lock().unwrap().deref() {
                Data::Boolean(b) => Ok(*b),
                _ => Ok(false),
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
    #[cfg(feature = "Trace")]
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
            #[cfg(feature = "Trace")]
            TraceMode::STATES,
            2000u64,
            &final_expected_configuration,
        ));
    }
}
