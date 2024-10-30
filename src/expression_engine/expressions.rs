//! Implementation of a simple expression parser.

use crate::datamodel::{Data, ToAny};
use std::fmt::Debug;

pub trait ExpressionContext {
    fn get_null(&self) -> &Data;
    fn get_data(&self, key : &String ) -> Option<&Data>;
    fn set_data(&mut self, key : String, data : Data );
    fn execute_action(&mut self, key : &String, arguments: &[Data]) -> Result<Data, String>;
}

#[derive(Debug, PartialEq)]
pub enum ExpressionResult {
    Data(Data),
    VariableReference(Vec<String>),
    Error(String)
}

impl ExpressionResult {
    pub fn get_value(&self, context: &dyn ExpressionContext) -> Result<Data,String> {
        match self {
            ExpressionResult::Data(data) => {
                Ok(data.clone())
            }
            ExpressionResult::VariableReference(r) => {
                let mut rx : Option<&Data> = None;
                for s in r {
                    if rx.is_some() {
                        match rx.unwrap() {
                            // Data::Array(_) => {}
                            Data::Map(map) => {
                                match map.get(s) {
                                    None => {
                                        rx = None;
                                        break;
                                    }
                                    Some(data) => {
                                        let _ = rx.insert(data);
                                    }
                                }
                            }
                            _ => {
                                rx = None;
                                break;
                            }
                        }
                    } else {
                        match context.get_data(s) {
                            None => {
                                break;
                            }
                            Some(data) => {
                                let _ = rx.insert(data);
                            }
                        }
                    }
                }
                if rx.is_some() {
                    Ok( rx.unwrap().clone() )
                } else {
                    Err("Reference not found".to_string())
                }
            }
            ExpressionResult::Error(err) => {
                Err(err.clone())
            }
        }
    }
}

pub trait Expression : ToAny + Debug {
    fn execute(&self, context: &mut dyn ExpressionContext) -> ExpressionResult;
    fn assign(&self, context: &mut dyn ExpressionContext, value : &Data ) -> ExpressionResult;
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
    pub fn new(array : Vec<Box<dyn Expression>> ) -> ExpressionArray {
        ExpressionArray {
            array,
        }
    }
}

impl Expression for ExpressionArray {
    fn execute(&self, context: &mut dyn ExpressionContext) -> ExpressionResult {
        let mut v = Vec::with_capacity(self.array.len());
        for item in &self.array {
            match item.execute(context).get_value(context) {
                Ok(data) => {
                    v.push( data );
                }
                Err(err) => {
                    return ExpressionResult::Error(err);
                }
            }
        }
        ExpressionResult::Data(Data::Array(v))
    }

    fn assign(&self, _context: &mut dyn ExpressionContext, _value : &Data ) -> ExpressionResult {
        ExpressionResult::Error("Can't assign a value to a method".to_string())
    }
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
    fn execute(&self, context: &mut dyn ExpressionContext) -> ExpressionResult {
        let mut v = Vec::with_capacity(self.arguments.len());
        for arg in &self.arguments {
            match arg.execute(context).get_value(context) {
                Ok(data) => {
                    v.push( data );
                }
                Err(err) => {
                    return ExpressionResult::Error(err);
                }
            }
        }
        match context.execute_action(&self.method, v.as_slice()) {
            Ok(result) => {
                ExpressionResult::Data(result)
            }
            Err(err) => {
                ExpressionResult::Error(err)
            }
        }
    }

    fn assign(&self, _context: &mut dyn ExpressionContext, _value : &Data ) -> ExpressionResult {
        ExpressionResult::Error("Can't assign a value to a method".to_string())
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
    fn execute(&self, _context: &mut dyn ExpressionContext) -> ExpressionResult {
        ExpressionResult::Data(self.data.clone())
    }

    fn assign(&self, _context: &mut dyn ExpressionContext, _value: &Data) -> ExpressionResult {
        ExpressionResult::Error("Can't assign a value to a Constant".to_string())
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
    fn execute(&self, context: &mut dyn ExpressionContext) -> ExpressionResult {
        match context.get_data(&self.name) {
            Some(_) => {
                ExpressionResult::VariableReference(vec!(self.name.clone()))
            }
            None => {
                ExpressionResult::Error(format!("Variable '{}' not found", self.name))
            }
        }
    }

    fn assign(&self, context: &mut dyn ExpressionContext, value: &Data) -> ExpressionResult {
        println!("assign {:?} = {}", self, value);
        context.set_data(self.name.clone(), value.clone());
        return ExpressionResult::Data(value.clone());
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
pub struct ExpressionMemberAccess {
    pub left: Box<dyn Expression>,
    pub member_name: String,
}

impl ExpressionMemberAccess {
    pub fn new(left: Box<dyn Expression>, member_name: String) -> ExpressionMemberAccess {
        ExpressionMemberAccess {
            left,
            member_name,
        }
    }
}

impl Expression for ExpressionMemberAccess {
    fn execute(&self, context: &mut dyn ExpressionContext) -> ExpressionResult {
        let left_result = self.left.execute(context);
        match left_result {
            ExpressionResult::Data(data) => {
                if let Data::Map(map)  = data{
                    match map.get(&self.member_name) {
                        None => {
                            ExpressionResult::Data(Data::Null())
                        }
                        Some(data) => {
                            ExpressionResult::Data(data.clone())
                        }
                    }
                } else {
                    ExpressionResult::Error(format!("Member '{}' not found", self.member_name))
                }
            }
            ExpressionResult::VariableReference(mut f1) => {
                f1.push( self.member_name.clone() );
                ExpressionResult::VariableReference(f1)
            }
            ExpressionResult::Error(err) => {
                ExpressionResult::Error(err)
            }
        }
    }

    fn assign(&self, context: &mut dyn ExpressionContext, value: &Data) -> ExpressionResult {
        let left_result = self.left.execute(context);
        match left_result {
            ExpressionResult::Data(data) => {
                if let Data::Map(mut map)  = data {
                    map.insert( self.member_name.clone(), value.clone() );
                    ExpressionResult::Data( value.clone() )
                } else {
                    ExpressionResult::Error("Failed".to_string())
                }
            }
            ExpressionResult::VariableReference(f1) => {
                let mut f2 = f1.clone();
                f2.push(self.member_name.clone());
                ExpressionResult::VariableReference( f2 )
            }
            ExpressionResult::Error(err) => {
                ExpressionResult::Error(err)
            }
        }
    }
}



#[derive(Debug)]
pub struct ExpressionAssign {
    pub left: Box<dyn Expression>,
    pub right: Box<dyn Expression>,
}

impl ExpressionAssign {
    pub fn new(left: Box<dyn Expression>, right: Box<dyn Expression>) -> ExpressionAssign {
        ExpressionAssign {
            left,
            right,
        }
    }
}

impl Expression for ExpressionAssign {

    fn execute(&self, context: &mut dyn ExpressionContext) -> ExpressionResult {
        let right_result = self.right.execute(context).get_value(context);
        match right_result {
            Ok(data) => {
                self.left.assign(context, &data )
            }
            Err(err) => {
                ExpressionResult::Error(err)
            }
        }
    }

    fn assign(&self, _context: &mut dyn ExpressionContext, _value: &Data) -> ExpressionResult {
        todo!()
    }
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
    fn execute(&self, context: &mut dyn ExpressionContext) -> ExpressionResult {
        let left_result = self.left.execute(context).get_value(context);
        let right_result = self.right.execute(context).get_value(context);
        println!("execute {:?} {:?}  {:?}", left_result, self.operator, right_result );
        match right_result {
            Ok(rd) => {
                match left_result {
                    Ok(d) => {
                        let result_data = d.operation( self.operator.clone(), &rd );
                        if let Data::Error(err) = result_data {
                            ExpressionResult::Error(err)
                        } else {
                            ExpressionResult::Data(result_data)
                        }
                    }
                    Err(err) => {
                        ExpressionResult::Error(err)
                    }
                }
            }
            Err(err) => {
                ExpressionResult::Error(err)
            }
        }
    }

    fn assign(&self, _context: &mut dyn ExpressionContext, _value: &Data) -> ExpressionResult {
        todo!()
    }
}
