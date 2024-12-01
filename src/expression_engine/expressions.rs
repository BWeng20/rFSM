//! Implementation of a simple expression parser.

use std::fmt::Debug;

use crate::datamodel::{Data, DataId, GlobalDataLock, ToAny};
use crate::expression_engine::expressions::ExpressionResult::{Error, Reference, Value};

#[derive(Debug,PartialEq)]
pub enum ExpressionResult {
    Error(String),
    Value(Data),
    Reference(DataId),
}

pub trait Expression: ToAny + Debug {
    fn execute(&self, context: &mut GlobalDataLock, allow_undefined: bool) -> ExpressionResult;
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
            match  item.execute(context, allow_undefined)
            {
                Error(err) => {
                    return Error(err);
                }
                Value(val) => {
                    todo!();
                    // v.push(val);
                }
                Reference(id) => {
                    v.push(id);
                }
            }
        }
        Value(Data::Array(v))
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
                Value(_) => {
                    todo!();
                }
                Reference(id) => {
                    v.push(id);
                }
            };
        }
        todo!()
        // context.execute_action(&self.method, v.as_slice())
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
    fn execute(&self, _context: &mut GlobalDataLock, allow_undefined: bool) -> ExpressionResult {
        Value(self.data.clone())
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
        match context.data.get_id(&self.name) {
            Some(id) => {
                ExpressionResult::Reference(id)
            }
            None => {
                ExpressionResult::Error(format!("Variable '{}' not found", self.name))
            }
        }
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
        todo!()
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
        let right_result = self.right.execute(context, allow_undefined);
        let left_result = self.left.execute(context, allow_undefined);
        println!("Assign {:?} <- {:?}", left_result, right_result);
        match left_result {
            Error(err) => Error(err),
            Value(v) => Error("Can't assign to value, left expression must reference so some variable".to_string()),
            Reference(id) => {
                let value = match right_result {
                    Error(err) => return Error(err),
                    Value(val) => {
                        val.clone()
                    }
                    Reference(id) => match context.data.get_by_id(id) {
                        None => {
                            return Error("Can assign right expression".to_string());
                        },
                        Some(right_val) => {
                            right_val.clone()
                        }
                    }
                };
                context.data.set_by_id(id, value);
                Reference(id)
            },
        }
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
        let right_result = match self.right.execute(context, allow_undefined) {
            Error(err) => {
                return Error(err);
            }
            Value(val) => { val.clone() }
            Reference(id) => {
                context.data.get_by_id(id).unwrap().clone()
            }
        };
        let left_result = self.left.execute(context, true);
        todo!()
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
            Error(err) => { return Error(err); }
            Value(val) => { val.clone() }
            Reference(id) => {
                // TODO
                context.data.get_by_id(id).unwrap().clone()
            }
        };
        let right_result = match self.right.execute(context, allow_undefined) {
            Error(err) => { return Error(err); }
            Value(val) => { val.clone() }
            Reference(id) => {
                // TODO
                context.data.get_by_id(id).unwrap().clone()
            }
        };
        println!(
            "execute {:?} {:?}  {:?}",
            left_result, self.operator, right_result
        );
        ExpressionResult::Value(left_result.operation(self.operator.clone(), &right_result))
    }
}

#[cfg(test)]
mod tests {
    use crate::datamodel::GlobalDataArc;
    use crate::expression_engine::datamodel::RFsmExpressionDatamodel;
    use crate::expression_engine::parser::ExpressionParser;

    #[test]
    fn can_assign_members() {
        let mut ec = RFsmExpressionDatamodel::new(GlobalDataArc::new());
        let rs = ExpressionParser::execute("a.b = 2".to_string(), &mut ec.global_data.lock());

        println!("{:?}", rs);
    }
}
