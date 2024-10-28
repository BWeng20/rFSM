//! Implementation of a simple expression parser.

use crate::datamodel::Data;
use std::fmt::Debug;
use crate::fsm::GlobalData;

pub trait Expression : Debug {
    fn execute(&self, data: &mut GlobalData) -> Data;
}

#[derive(Debug)]
pub struct ExpressionMethod {
    pub arguments: Vec<Box<dyn Expression>>,
    pub method: String
}

impl ExpressionMethod {
    pub fn new(method: &str, arguments : Vec<Box<dyn Expression>> ) -> ExpressionMethod {
        ExpressionMethod {
            arguments,
            method: method.to_string(),
        }
    }
}

impl Expression for ExpressionMethod {
    fn execute(&self, data: &mut GlobalData) -> Data {
        let mut v = Vec::with_capacity(self.arguments.len());
        for arg in &self.arguments {
            v.push( arg.execute(data) );
        }
        match data.actions.lock().get(self.method.as_str() ) {
            None => {
                todo!()
            }
            Some(action) => {
                action.execute( v.as_slice(), data).unwrap()
            }
        }
    }
}

#[derive(Debug)]
pub struct ExpressionVariable {
    pub name: String,
}

impl crate::expressions::ExpressionVariable {
    pub fn new(name: &str) -> crate::expressions::ExpressionVariable {
        crate::expressions::ExpressionVariable {
            name: name.to_string(),
        }
    }
}

impl Expression for crate::expressions::ExpressionVariable {
    fn execute(&self, data: &mut GlobalData) -> Data {
        match data.environment.get(self.name.as_str()) {
            None => Data::Null(),
            Some(d) => {
                d.clone()
            }
        }
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
    fn execute(&self, _data: &mut GlobalData) -> Data {
        self.data.clone()
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
    Equal,
    NotEqual,
    Modulo,
    Not,
}

#[derive(Debug)]
pub struct ExpressionOperator {
    pub operator : Operator,
    pub left: Box<dyn Expression>,
    pub right: Box<dyn Expression>,
}

impl ExpressionOperator {
    pub fn new(op : Operator, left: Box<dyn Expression>, right: Box<dyn Expression>) -> ExpressionOperator {
        ExpressionOperator {
            left,
            right,
            operator : op
        }
    }
}

impl Expression for ExpressionOperator {
    fn execute(&self, data: &mut GlobalData) -> Data {
        let left_result = self.left.execute(data);
        let right_result = self.right.execute(data);
        left_result.operation( self.operator.clone(), &right_result)
    }
}