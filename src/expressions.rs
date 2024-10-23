//! Implementation of a simple expression parser.

use crate::datamodel::Data;
use std::collections::HashMap;
use std::fmt::{Debug, Display};
use std::option::Option;

#[derive(Default)]
pub struct ExpressionData {
    #[allow(dead_code)]
    pub methods: HashMap<String, ExpressionMethod>,
    pub data: HashMap<String, Data>,
    pub left_expression: Data,
}

impl ExpressionData {
    pub fn get(&self, name: &str) -> Option<&Data> {
        self.data.get(name)
    }
}

pub trait Expression {
    fn execute(&self, data: &mut ExpressionData);
}

pub type ExpressionMethodCall = fn(&ExpressionMethod, &mut ExpressionData) -> Data;

pub struct ExpressionMethod {
    pub arguments: Vec<Box<dyn Expression>>,
    pub call: ExpressionMethodCall,
}

impl ExpressionMethod {
    pub fn new(f: ExpressionMethodCall) -> ExpressionMethod {
        ExpressionMethod {
            arguments: Vec::new(),
            call: f,
        }
    }
}

impl Expression for ExpressionMethod {
    fn execute(&self, data: &mut ExpressionData) {
        data.left_expression = (self.call)(&self, data)
    }
}

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
    fn execute(&self, data: &mut ExpressionData) {
        match data.get(self.name.as_str()) {
            None => data.left_expression = Data::Null(),
            Some(d) => {
                data.left_expression = d.clone();
            }
        }
    }
}

pub struct ConstantExpression {
    pub data: Data,
}

impl ConstantExpression {
    pub fn new(d: Data) -> ConstantExpression {
        ConstantExpression { data: d }
    }
}

impl Expression for ConstantExpression {
    fn execute(&self, data: &mut ExpressionData) {
        data.left_expression = self.data.clone();
    }
}

pub struct SubExpression {
    pub sequence: Vec<Box<dyn Expression>>,
}

impl crate::expressions::SubExpression {
    pub fn new() -> crate::expressions::SubExpression {
        crate::expressions::SubExpression {
            sequence: Vec::new(),
        }
    }
}

impl Expression for crate::expressions::SubExpression {
    fn execute(&self, data: &mut ExpressionData) {
        for e in &self.sequence {
            e.execute(data);
        }
    }
}
