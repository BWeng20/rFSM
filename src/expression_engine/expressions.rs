//! Implementation of a simple expression parser.

use std::fmt::Debug;
use std::ops::{Deref, DerefMut};
use std::sync::Arc;

#[cfg(feature = "Debug")]
use log::debug;

use crate::datamodel::{create_data_arc, numeric_to_integer, Data, DataArc, GlobalDataLock, ToAny};

pub type ExpressionResult = Result<DataArc, String>;

pub trait Expression: ToAny + Debug {
    fn execute(&self, context: &mut GlobalDataLock, allow_undefined: bool) -> ExpressionResult;
    fn is_assignable(&self) -> bool;
    fn get_copy(&self) -> Box<dyn Expression>;
}

pub fn get_expression_as<T: 'static>(ec: &dyn Expression) -> Option<&T> {
    let va = ec.as_any();
    va.downcast_ref::<T>()
}

#[derive(Debug)]
pub struct ExpressionArray {
    pub array: Vec<Box<dyn Expression>>,
}

impl ExpressionArray {
    pub fn new(array: Vec<Box<dyn Expression>>) -> ExpressionArray {
        ExpressionArray { array }
    }
}

impl Expression for ExpressionArray {
    fn execute(&self, context: &mut GlobalDataLock, allow_undefined: bool) -> ExpressionResult {
        let mut v = Vec::with_capacity(self.array.len());
        for item in &self.array {
            match item.execute(context, allow_undefined) {
                Err(err) => {
                    return Err(err);
                }
                Ok(val) => {
                    v.push(val);
                }
            }
        }
        Ok(create_data_arc(Data::Array(v)))
    }

    fn is_assignable(&self) -> bool {
        false
    }

    fn get_copy(&self) -> Box<dyn Expression> {
        let mut ac = Vec::with_capacity(self.array.len());
        for e in &self.array {
            ac.push(e.get_copy())
        }
        Box::new(ExpressionArray::new(ac))
    }
}

#[derive(Debug)]
pub struct ExpressionMethod {
    pub arguments: Vec<Box<dyn Expression>>,
    pub method: String,
}

impl ExpressionMethod {
    pub fn new(method: &str, arguments: Vec<Box<dyn Expression>>) -> ExpressionMethod {
        ExpressionMethod {
            arguments,
            method: method.to_string(),
        }
    }

    pub fn execute_with_arguments(&self, arguments: &[Data], context: &mut GlobalDataLock) -> ExpressionResult {
        match context
            .actions
            .execute(self.method.as_str(), arguments, context)
        {
            Ok(rdata) => Ok(create_data_arc(rdata)),
            Err(err) => Err(err),
        }
    }

    fn eval_arguments(&self, v: &mut Vec<Data>, context: &mut GlobalDataLock) -> Result<(), String> {
        for arg in &self.arguments {
            v.push(match arg.execute(context, false) {
                Ok(data_arc) => data_arc.lock().unwrap().clone(),
                Err(err) => Data::Error(err),
            });
        }
        Ok(())
    }

    pub fn get_copy(&self) -> Box<ExpressionMethod> {
        let mut av = Vec::with_capacity(self.arguments.len());
        for a in &self.arguments {
            av.push(a.get_copy());
        }
        Box::new(ExpressionMethod::new(self.method.as_str(), av))
    }
}

impl Expression for ExpressionMethod {
    fn execute(&self, context: &mut GlobalDataLock, _: bool) -> ExpressionResult {
        let mut v = Vec::with_capacity(self.arguments.len());
        self.eval_arguments(&mut v, context)?;
        self.execute_with_arguments(v.as_slice(), context)
    }

    fn is_assignable(&self) -> bool {
        false
    }

    fn get_copy(&self) -> Box<dyn Expression> {
        self.get_copy()
    }
}

#[derive(Debug)]
pub struct ExpressionConstant {
    pub data: Data,
}

impl ExpressionConstant {
    pub fn new(d: Data) -> ExpressionConstant {
        ExpressionConstant { data: d }
    }
}

impl Expression for ExpressionConstant {
    fn execute(&self, _context: &mut GlobalDataLock, _allow_undefined: bool) -> ExpressionResult {
        Ok(create_data_arc(self.data.clone()))
    }

    fn is_assignable(&self) -> bool {
        false
    }

    fn get_copy(&self) -> Box<dyn Expression> {
        Box::new(ExpressionConstant::new(self.data.clone()))
    }
}

#[derive(Debug)]
pub struct ExpressionVariable {
    pub name: String,
}

impl ExpressionVariable {
    pub fn new(name: &str) -> ExpressionVariable {
        ExpressionVariable {
            name: name.to_string(),
        }
    }
}

impl Expression for ExpressionVariable {
    fn execute(&self, context: &mut GlobalDataLock, allow_undefined: bool) -> ExpressionResult {
        match context.data.get(&self.name) {
            Some(value) => {
                #[cfg(feature = "Debug")]
                debug!("ExpressionVariable::execute: {} = {}", self.name, value);
                Ok(value.clone())
            }
            None => {
                if allow_undefined {
                    #[cfg(feature = "Debug")]
                    debug!("ExpressionVariable::execute: init {} = None", self.name);
                    context.data.set_undefined(self.name.clone(), Data::None());
                    Ok(context.data.get(&self.name).unwrap())
                } else {
                    Err(format!("Variable '{}' not found", self.name))
                }
            }
        }
    }

    fn is_assignable(&self) -> bool {
        true
    }

    fn get_copy(&self) -> Box<dyn Expression> {
        Box::new(ExpressionVariable::new(self.name.as_str()))
    }
}

#[derive(PartialEq, Debug, Clone)]
#[repr(u8)]
pub enum Operator {
    Multiply,
    Divide,
    Plus,
    Minus,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    Assign,
    AssignUndefined,
    Equal,
    NotEqual,

    /// C-like modulus (mathematically the remainder) function.
    Modulus,
    Not,
}

#[derive(Debug)]
pub struct ExpressionIndex {
    pub left: Box<dyn Expression>,
    pub index: Box<dyn Expression>,
}

impl ExpressionIndex {
    pub fn new(left: Box<dyn Expression>, index: Box<dyn Expression>) -> ExpressionIndex {
        ExpressionIndex { left, index }
    }
}

impl Expression for ExpressionIndex {
    fn execute(&self, context: &mut GlobalDataLock, allow_undefined: bool) -> ExpressionResult {
        let left_result = self.left.execute(context, allow_undefined);
        let index_result = self.index.execute(context, allow_undefined);
        match (left_result, index_result) {
            (Err(err), _) => Err(err),
            (_, Err(err)) => Err(err),
            (Ok(left_value), Ok(index_value)) => {
                let mut data_ref = left_value.lock().unwrap();
                let data = data_ref.deref_mut();
                match data {
                    Data::Integer(_)
                    | Data::Double(_)
                    | Data::String(_)
                    | Data::Boolean(_)
                    | Data::Source(_)
                    | Data::Null()
                    | Data::None() => Err(format!("Can't apply index on value '{}'", data)),
                    Data::Map(m) => {
                        let index_guard = index_value.lock().unwrap();
                        let index_data = index_guard.deref();
                        match index_data {
                            Data::Source(key) | Data::String(key) => match m.get(key) {
                                None => {
                                    if allow_undefined {
                                        m.insert(key.clone(), create_data_arc(Data::None()));
                                        Ok(m.get(key).unwrap().clone())
                                    } else {
                                        Err(format!("Index {} not found", key))
                                    }
                                }
                                Some(member) => Ok(member.clone()),
                            },
                            Data::Boolean(_)
                            | Data::Array(_)
                            | Data::Map(_)
                            | Data::Integer(_)
                            | Data::Double(_)
                            | Data::Error(_)
                            | Data::Null()
                            | Data::None() => Err(format!("Illegal index type '{}'", index_value)),
                        }
                    }
                    Data::Array(m) => match numeric_to_integer(index_value.lock().unwrap().deref()) {
                        Some(index) => match m.get(index as usize) {
                            None => Err(format!("Index not found: {} (len={})", index, m.len())),
                            Some(value) => Ok(value.clone()),
                        },
                        None => Err(format!("Illegal index type '{}'", index_value)),
                    },
                    Data::Error(err) => Err(err.clone()),
                }
            }
        }
    }

    fn is_assignable(&self) -> bool {
        true
    }

    fn get_copy(&self) -> Box<dyn Expression> {
        Box::new(ExpressionIndex::new(
            self.left.get_copy(),
            self.index.get_copy(),
        ))
    }
}

#[derive(Debug)]
pub struct ExpressionMemberAccess {
    pub left: Box<dyn Expression>,
    pub member_name: String,
}

impl ExpressionMemberAccess {
    pub fn new(left: Box<dyn Expression>, member_name: String) -> ExpressionMemberAccess {
        ExpressionMemberAccess { left, member_name }
    }
}

impl Expression for ExpressionMemberAccess {
    fn execute(&self, context: &mut GlobalDataLock, allow_undefined: bool) -> ExpressionResult {
        match self.left.execute(context, allow_undefined) {
            Err(err) => Err(err),
            Ok(val) => {
                let mut data_ref = val.lock().unwrap();
                let data = data_ref.deref_mut();
                match data {
                    Data::Integer(_)
                    | Data::Double(_)
                    | Data::String(_)
                    | Data::Boolean(_)
                    | Data::Array(_)
                    | Data::Source(_)
                    | Data::Null()
                    | Data::None() => Err(format!("Value '{}' has no members", data)),
                    Data::Map(m) => match m.get(&self.member_name) {
                        None => {
                            if allow_undefined {
                                m.insert(self.member_name.clone(), create_data_arc(Data::None()));
                                Ok(m.get(&self.member_name).unwrap().clone())
                            } else {
                                Err(format!("Member {} not found", self.member_name))
                            }
                        }
                        Some(member) => Ok(member.clone()),
                    },
                    Data::Error(err) => Err(err.clone()),
                }
            }
        }
    }

    fn is_assignable(&self) -> bool {
        true
    }

    fn get_copy(&self) -> Box<dyn Expression> {
        Box::new(ExpressionMemberAccess::new(
            self.left.get_copy(),
            self.member_name.clone(),
        ))
    }
}

#[derive(Debug)]
pub struct ExpressionAssign {
    pub left: Box<dyn Expression>,
    pub right: Box<dyn Expression>,
}

impl ExpressionAssign {
    pub fn new(left: Box<dyn Expression>, right: Box<dyn Expression>) -> ExpressionAssign {
        ExpressionAssign { left, right }
    }
}

impl Expression for ExpressionAssign {
    fn execute(&self, context: &mut GlobalDataLock, allow_undefined: bool) -> ExpressionResult {
        if self.left.is_assignable() {
            let right_result = self.right.execute(context, false);
            let left_result = self.left.execute(context, allow_undefined);

            match left_result {
                Err(err) => Err(err),
                Ok(v) => match right_result {
                    Err(err) => Err(err),
                    Ok(right_arc) => {
                        let right_guard = right_arc.lock().unwrap();
                        match right_guard.deref() {
                            Data::Integer(_)
                            | Data::Double(_)
                            | Data::String(_)
                            | Data::Boolean(_)
                            | Data::Array(_)
                            | Data::Map(_)
                            | Data::Null()
                            | Data::Source(_) => {
                                if v.is_readonly() {
                                    Err(format!("Can't set read-only {v}"))
                                } else {
                                    right_guard
                                        .deref()
                                        .clone_into(v.lock().unwrap().deref_mut());
                                    Ok(v.clone())
                                }
                            }
                            Data::Error(_) | Data::None() => Err(format!("Can't assign from '{}'", right_guard)),
                        }
                    }
                },
            }
        } else {
            Err("Can't assign to that".to_string())
        }
    }

    fn is_assignable(&self) -> bool {
        false
    }

    fn get_copy(&self) -> Box<dyn Expression> {
        Box::new(ExpressionAssign::new(
            self.left.get_copy(),
            self.right.get_copy(),
        ))
    }
}

#[derive(Debug)]
pub struct ExpressionAssignUndefined {
    pub left: Box<dyn Expression>,
    pub right: Box<dyn Expression>,
}

impl ExpressionAssignUndefined {
    pub fn new(left: Box<dyn Expression>, right: Box<dyn Expression>) -> ExpressionAssignUndefined {
        ExpressionAssignUndefined { left, right }
    }
}

impl Expression for ExpressionAssignUndefined {
    fn execute(&self, context: &mut GlobalDataLock, allow_undefined: bool) -> ExpressionResult {
        if self.left.is_assignable() {
            let right_result = match self.right.execute(context, allow_undefined) {
                Err(err) => {
                    return Err(err);
                }
                Ok(val) => val,
            };
            let left_result = self.left.execute(context, true);
            match left_result {
                Err(err) => Err(err),
                Ok(left_value) => {
                    right_result
                        .lock()
                        .unwrap()
                        .deref()
                        .clone_into(left_value.lock().unwrap().deref_mut());
                    Ok(left_value.clone())
                }
            }
        } else {
            Err(format!("Can't assign to {:?}", self.left))
        }
    }

    fn is_assignable(&self) -> bool {
        false
    }

    fn get_copy(&self) -> Box<dyn Expression> {
        Box::new(ExpressionAssignUndefined::new(
            self.left.get_copy(),
            self.right.get_copy(),
        ))
    }
}

#[derive(Debug)]
pub struct ExpressionOperator {
    pub operator: Operator,
    pub left: Box<dyn Expression>,
    pub right: Box<dyn Expression>,
}

impl ExpressionOperator {
    pub fn new(op: Operator, left: Box<dyn Expression>, right: Box<dyn Expression>) -> ExpressionOperator {
        ExpressionOperator {
            left,
            right,
            operator: op,
        }
    }
}

impl Expression for ExpressionOperator {
    fn execute(&self, context: &mut GlobalDataLock, allow_undefined: bool) -> ExpressionResult {
        #[cfg(feature = "Debug")]
        {
            debug!("ExpressionOperator::execute:");
            context.data.dump();
        }
        let left_result = match self.left.execute(context, allow_undefined) {
            Err(err) => {
                return Err(err);
            }
            Ok(val) => val.clone(),
        };
        let right_result = match self.right.execute(context, allow_undefined) {
            Err(err) => {
                return Err(err);
            }
            Ok(val) => val.clone(),
        };
        #[cfg(feature = "Debug")]
        debug!(
            "ExpressionOperator::execute: <{:?}={}> {:?} <{:?}={}>",
            self.left, left_result, self.operator, self.right, right_result
        );
        let result_data = if Arc::ptr_eq(&left_result.arc, &right_result.arc) {
            // Same object, we have to clone the content at least for one side to avoid deadlock.
            let left_data = left_result.lock().unwrap().clone();
            left_data.operation(self.operator.clone(), right_result.lock().unwrap().deref())
        } else {
            left_result
                .lock()
                .unwrap()
                .operation(self.operator.clone(), right_result.lock().unwrap().deref())
        };
        Ok(create_data_arc(result_data))
    }

    fn is_assignable(&self) -> bool {
        false
    }

    fn get_copy(&self) -> Box<dyn Expression> {
        Box::new(ExpressionOperator::new(
            self.operator.clone(),
            self.left.get_copy(),
            self.right.get_copy(),
        ))
    }
}

#[derive(Debug)]
pub struct ExpressionNot {
    pub right: Box<dyn Expression>,
}
impl ExpressionNot {
    pub fn new(right: Box<dyn Expression>) -> ExpressionNot {
        ExpressionNot { right }
    }
}

impl Expression for ExpressionNot {
    fn execute(&self, context: &mut GlobalDataLock, allow_undefined: bool) -> ExpressionResult {
        match self.right.execute(context, allow_undefined) {
            Err(err) => Err(err),
            Ok(val) => match val.lock() {
                Ok(val_guard) => match val_guard.deref() {
                    Data::Boolean(bool_val) => Ok(create_data_arc(Data::Boolean(!bool_val))),
                    _ => Err("'!' can only be applied on boolean expressions.".to_string()),
                },
                Err(err) => Err(err.to_string()),
            },
        }
    }

    fn is_assignable(&self) -> bool {
        false
    }

    fn get_copy(&self) -> Box<dyn Expression> {
        Box::new(ExpressionNot::new(self.right.get_copy()))
    }
}

#[cfg(test)]
mod tests {
    use crate::datamodel::{create_data_arc, create_global_data_arc, Data};
    use crate::expression_engine::datamodel::RFsmExpressionDatamodel;
    use crate::expression_engine::expressions::ExpressionResult;
    use crate::expression_engine::parser::ExpressionParser;
    use std::collections::HashMap;

    #[test]
    fn can_assign_members() {
        let ec = RFsmExpressionDatamodel::new(create_global_data_arc());
        let mut data_members = HashMap::new();
        data_members.insert("b".to_string(), create_data_arc(Data::Null()));
        let mut gdata = ec.global_data.lock().unwrap();
        gdata
            .data
            .set_undefined("a".to_string(), Data::Map(data_members));
        let rs = ExpressionParser::execute("a.b = 2".to_string(), &mut gdata);

        println!("{:?}", rs);
        assert_eq!(rs, ExpressionResult::Ok(create_data_arc(Data::Integer(2))));
    }

    #[test]
    fn can_assign_variable() {
        let ec = RFsmExpressionDatamodel::new(create_global_data_arc());
        let rs = ExpressionParser::execute("a = 2".to_string(), &mut ec.global_data.lock().unwrap());

        println!("{:?}", rs);
    }
}
