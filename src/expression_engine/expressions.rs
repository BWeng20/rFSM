//! Implementation of a simple expression parser.

use std::fmt::{Debug, Display, Formatter};
use std::ops::{Deref, DerefMut};

use crate::datamodel::{create_data_arc, Data, DataArc, GlobalDataLock, ToAny};
use crate::expression_engine::expressions::ExpressionResult::{Error, Value};

#[derive(Debug)]
pub enum ExpressionResult {
    Error(String),
    Value(DataArc),
}

impl Display for ExpressionResult {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Error(err) => {
                write!(f, "Error:{}", err)
            }
            Value(value) => {
                write!(f, "{}", value)
            }
        }
    }
}


impl PartialEq for ExpressionResult {
    fn eq(&self, other: &Self) -> bool {
        match self {
            Error(a) => {
                if let Error(b) = other {
                    a == b
                } else {
                    false
                }
            }
            Value(a) => {
                if let Value(b) = other {
                    a.lock().unwrap().deref() == b.lock().unwrap().deref()
                } else {
                    false
                }
            }
        }
    }
}

pub trait Expression: ToAny + Debug {
    fn execute(&self, context: &mut GlobalDataLock, allow_undefined: bool) -> ExpressionResult;
    fn is_assignable(&self) -> bool;
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
                Error(err) => {
                    return Error(err);
                }
                Value(val) => {
                    v.push(val);
                }
            }
        }
        Value(create_data_arc(Data::Array(v)))
    }

    fn is_assignable(&self) -> bool {
        false
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
}

impl Expression for ExpressionMethod {
    fn execute(&self, context: &mut GlobalDataLock, allow_undefined: bool) -> ExpressionResult {
        let mut v = Vec::with_capacity(self.arguments.len());
        for arg in &self.arguments {
            match arg.execute(context, allow_undefined) {
                Error(err) => {
                    return Error(err);
                }
                Value(data) => {
                    v.push(data);
                }
            };
        }
        todo!()
        // context.execute_action(&self.method, v.as_slice())
    }

    fn is_assignable(&self) -> bool {
        false
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
        Value(create_data_arc(self.data.clone()))
    }

    fn is_assignable(&self) -> bool {
        false
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
            Some(value) => ExpressionResult::Value(value.clone()),
            None => {
                if allow_undefined {
                    context.data.set_undefined(self.name.clone(), Data::None());
                    Value(context.data.get(&self.name).unwrap())
                } else {
                    Error(format!("Variable '{}' not found", self.name))
                }
            }
        }
    }

    fn is_assignable(&self) -> bool {
        true
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
    Modulo,
    Not,
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
            Error(err) => Error(err),
            Value(val) => match val.lock().unwrap().deref_mut() {
                Data::Integer(_)
                | Data::Double(_)
                | Data::String(_)
                | Data::Boolean(_)
                | Data::Array(_)
                | Data::Source(_)
                | Data::Null()
                | Data::None() => Error("Value has no members".to_string()),
                Data::Map(m) => match m.get(&self.member_name) {
                    None => {
                        if allow_undefined {
                            m.insert(self.member_name.clone(), create_data_arc(Data::None()));
                            Value(m.get(&self.member_name).unwrap().clone())
                        } else {
                            Error(format!("Member {} not found", self.member_name))
                        }
                    }
                    Some(member) => Value(member.clone()),
                },
                Data::Error(err) => Error(err.clone()),
            },
        }
    }

    fn is_assignable(&self) -> bool {
        true
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
            let right_result = self.right.execute(context, allow_undefined);
            let left_result = self.left.execute(context, false);
            println!("Assign {} <- {}", left_result, right_result);
            match left_result {
                Error(err) => Error(err),
                Value(v) => {
                    match right_result {
                        Error(err) => Error(err),
                        Value(vr) => {
                            *(v.lock().unwrap().deref_mut()) = vr.lock().unwrap().deref().clone();
                            Value(v.clone())
                        }
                    }
                },
            }
        } else {
            Error("Can't assign to that".to_string())
        }
    }

    fn is_assignable(&self) -> bool {
        false
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
                Error(err) => {
                    return Error(err);
                }
                Value(val) => val,
            };
            let left_result = self.left.execute(context, true);
            match left_result {
                Error(err) => Error(err),
                Value(left_value) => {
                    right_result
                        .lock()
                        .unwrap()
                        .deref()
                        .clone_into(left_value.lock().unwrap().deref_mut());
                    Value(left_value.clone())
                }
            }
        } else {
            Error(format!("Can't assign to {:?}", self.left ))
        }
    }

    fn is_assignable(&self) -> bool {
        false
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
        let left_result = match self.left.execute(context, allow_undefined) {
            Error(err) => {
                return Error(err);
            }
            Value(val) => val.clone(),
        };
        let right_result = match self.right.execute(context, allow_undefined) {
            Error(err) => {
                return Error(err);
            }
            Value(val) => val.clone(),
        };
        println!(
            "execute {} {:?}  {}",
            left_result, self.operator, right_result
        );
        let result_data = left_result
            .lock()
            .unwrap()
            .operation(self.operator.clone(), right_result.lock().unwrap().deref());
        ExpressionResult::Value(create_data_arc(result_data))
    }

    fn is_assignable(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use crate::datamodel::create_global_data_arc;
    use crate::expression_engine::datamodel::RFsmExpressionDatamodel;
    use crate::expression_engine::parser::ExpressionParser;

    #[test]
    fn can_assign_members() {
        let ec = RFsmExpressionDatamodel::new(create_global_data_arc());
        let rs = ExpressionParser::execute("a.b = 2".to_string(), &mut ec.global_data.lock().unwrap());

        println!("{:?}", rs);
    }

    #[test]
    fn can_assign_variable() {
        let ec = RFsmExpressionDatamodel::new(create_global_data_arc());
        let rs = ExpressionParser::execute("a = 2".to_string(), &mut ec.global_data.lock().unwrap());

        println!("{:?}", rs);
    }
}
