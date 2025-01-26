//! Implementation of a simple expression parser.

use crate::datamodel::{Data, GlobalDataLock};
use crate::expression_engine::expressions::{
    get_expression_as, Expression, ExpressionArray, ExpressionAssign, ExpressionAssignUndefined, ExpressionConstant,
    ExpressionIndex, ExpressionMap, ExpressionMemberAccess, ExpressionMethod, ExpressionNot, ExpressionOperator,
    ExpressionResult, ExpressionSequence, ExpressionVariable,
};
use crate::expression_engine::lexer::{ExpressionLexer, NumericToken, Operator, Token};

use std::fmt;
use std::fmt::{Debug, Display, Formatter};
use std::ops::Deref;

#[cfg(feature = "Debug")]
use crate::fsm::vec_to_string;

#[cfg(feature = "Debug")]
use crate::common::debug;

/// Static tool struct to process expressions.
pub struct ExpressionParser {}

/// Internal item for the parser stack.
enum ExpressionParserItem {
    SToken(Token),
    SExpression(Box<dyn Expression>),
}

impl Display for ExpressionParserItem {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        match self {
            ExpressionParserItem::SToken(t) => Debug::fmt(t, f),
            ExpressionParserItem::SExpression(e) => Debug::fmt(e, f),
        }
    }
}

impl ExpressionParser {
    /// Parse a member list, stops at the matching stop char
    #[allow(clippy::type_complexity)]
    fn parse_member_list(
        lexer: &mut ExpressionLexer,
        stop: char,
    ) -> Result<Vec<(Box<dyn Expression>, Box<dyn Expression>)>, String> {
        let mut r = Vec::new();
        let mut stop_c;
        loop {
            let (_stop_key, key_expression_option) = Self::parse_sub_expression(lexer, &[':', stop])?;
            match key_expression_option {
                None => {
                    if r.is_empty() {
                        // Special case: empty member list
                        break;
                    } else {
                        return Err("Error in member list".to_string());
                    }
                }
                Some(key_expression) => {
                    let (stop_val, value_expression_option) = Self::parse_sub_expression(lexer, &[',', stop])?;
                    stop_c = stop_val;
                    match value_expression_option {
                        None => {
                            return Err("Missing value expression in member list".to_string());
                        }
                        Some(value_expression) => {
                            r.push((key_expression, value_expression));
                        }
                    }
                }
            }
            if stop_c == stop {
                break;
            }
            if stop_c == '\0' {
                return Err(format!("Missing '{}'", stop));
            }
        }
        Ok(r)
    }

    /// Parse an argument list, stops at the matching stop char
    fn parse_argument_list(lexer: &mut ExpressionLexer, stop: char) -> Result<Vec<Box<dyn Expression>>, String> {
        let mut r = Vec::new();
        loop {
            let (stopc, expression) = Self::parse_sub_expression(lexer, &[',', stop])?;
            match expression {
                None => {
                    if r.is_empty() {
                        // Special case: empty argument list
                        break;
                    } else {
                        return Err("Error in argument list".to_string());
                    }
                }
                Some(e) => {
                    r.push(e);
                }
            }
            if stopc == stop {
                break;
            }
            if stopc == '\0' {
                return Err(format!("Missing '{}'", stop));
            }
        }
        Ok(r)
    }

    /// Parse an expression, returning a re-usable expression.
    pub fn parse(text: String) -> Result<Box<dyn Expression>, String> {
        let mut lexer = ExpressionLexer::new(text);
        let (_, expression) = Self::parse_sub_expression(&mut lexer, &['\0'])?;
        match expression {
            None => Err("Failed to parse".to_string()),
            Some(e) => Ok(e),
        }
    }

    /// Parses and executes an expression.\
    /// If possible, please use "parse" and re-use the parsed expressions.
    pub fn execute_str(source: &str, context: &mut GlobalDataLock) -> ExpressionResult {
        Self::execute(source.to_string(), context)
    }

    /// Parses and executes an expression.\
    /// If possible, please use "parse" and re-use the parsed expressions.
    pub fn execute(source: String, context: &mut GlobalDataLock) -> ExpressionResult {
        #[cfg(feature = "Debug")]
        debug!("ExpressionParser::execute: {}", source);
        let parser_result = Self::parse(source);
        let r = match parser_result {
            Ok(v) => v.execute(context, false),
            Err(err) => ExpressionResult::Err(err),
        };
        #[cfg(feature = "Debug")]
        debug!("ExpressionParser::execute: result {:?}", r);
        r
    }

    fn parse_sub_expression(
        lexer: &mut ExpressionLexer,
        stops: &[char],
    ) -> Result<(char, Option<Box<dyn Expression>>), String> {
        // Translate the lexer tokens and put them to the stack. Resolve method calls and sub-expressions.
        // The result will be a stack sequence of identifier / operators / expressions.
        // All remaining "Identifier" are variables.
        let mut expressions = Vec::new();
        let mut stack: Vec<ExpressionParserItem> = Vec::new();
        let mut stop = '\0';
        loop {
            let t = lexer.next_token_with_stop(stops);
            match &t {
                Token::EOE => {
                    break;
                }
                Token::Null() => {
                    stack.push(ExpressionParserItem::SExpression(Box::new(
                        ExpressionConstant::new(Data::Null()),
                    )));
                }
                Token::TString(text) => {
                    stack.push(ExpressionParserItem::SExpression(Box::new(
                        ExpressionConstant::new(Data::String(text.clone())),
                    )));
                }
                Token::Boolean(v) => {
                    stack.push(ExpressionParserItem::SExpression(Box::new(
                        ExpressionConstant::new(Data::Boolean(*v)),
                    )));
                }
                Token::Number(v) => {
                    stack.push(ExpressionParserItem::SExpression(Box::new(
                        ExpressionConstant::new(match v {
                            NumericToken::Integer(i) => Data::Integer(*i),
                            NumericToken::Double(i) => Data::Double(*i),
                        }),
                    )));
                }
                Token::Identifier(_) => {
                    stack.push(ExpressionParserItem::SToken(t));
                }
                Token::Operator(_) => {
                    stack.push(ExpressionParserItem::SToken(t));
                }
                Token::Bracket(br) => match br {
                    '(' => {
                        let si = stack.pop();
                        match si {
                            None => {
                                let (_, se) = Self::parse_sub_expression(lexer, &[')'])?;
                                match se {
                                    None => {}
                                    Some(sev) => {
                                        stack.push(ExpressionParserItem::SExpression(sev));
                                    }
                                }
                            }
                            Some(si) => match si {
                                ExpressionParserItem::SToken(token) => match token {
                                    Token::Null()
                                    | Token::Separator(_)
                                    | Token::Bracket(_)
                                    | Token::Boolean(_)
                                    | Token::TString(_)
                                    | Token::Number(_) => {
                                        return Result::Err(format!("Unexpected '{}'", br));
                                    }
                                    Token::Identifier(id) => {
                                        let v = Self::parse_argument_list(lexer, ')')?;
                                        let x = Box::new(ExpressionMethod::new(id.as_str(), v));
                                        stack.push(ExpressionParserItem::SExpression(x));
                                    }
                                    Token::Operator(_) => {
                                        stack.push(ExpressionParserItem::SToken(token));
                                        let (_, se) = Self::parse_sub_expression(lexer, &[')'])?;
                                        match se {
                                            None => {}
                                            Some(sev) => {
                                                stack.push(ExpressionParserItem::SExpression(sev));
                                            }
                                        }
                                    }
                                    Token::Error(_) => {}
                                    Token::EOE => {}
                                    Token::ExpressionSeparator() => {}
                                },
                                ExpressionParserItem::SExpression(_) => {
                                    return Result::Err(format!("Unexpected '{}'", br));
                                }
                            },
                        }
                    }
                    '[' => {
                        let si = stack.pop();
                        let new_stack_item: Box<dyn Expression> = match si {
                            None => {
                                let v = Self::parse_argument_list(lexer, ']')?;
                                Box::new(ExpressionArray::new(v))
                            }
                            Some(si) => match si {
                                ExpressionParserItem::SToken(token) => match token {
                                    Token::Null()
                                    | Token::Separator(_)
                                    | Token::Bracket(_)
                                    | Token::Boolean(_)
                                    | Token::TString(_)
                                    | Token::Number(_) => {
                                        return Result::Err(format!("Unexpected '{}'", br));
                                    }
                                    Token::Identifier(id) => {
                                        let mut v = Self::parse_argument_list(lexer, ']')?;
                                        if v.len() != 1 {
                                            return Result::Err(
                                                "index operator '[]' allows only one argument".to_string(),
                                            );
                                        }
                                        Box::new(ExpressionIndex::new(
                                            Box::new(ExpressionVariable::new(id.as_str())),
                                            v.remove(0),
                                        ))
                                    }
                                    Token::Operator(_) => {
                                        // Put token back on stack.
                                        stack.push(ExpressionParserItem::SToken(token));
                                        let v = Self::parse_argument_list(lexer, ']')?;
                                        Box::new(ExpressionArray::new(v))
                                    }
                                    _ => {
                                        return Result::Err(format!("Internal Error at '{}'", br));
                                    }
                                },
                                ExpressionParserItem::SExpression(expression) => {
                                    let mut v = Self::parse_argument_list(lexer, ']')?;
                                    if v.len() != 1 {
                                        return Result::Err("index operator '[]' allows only one argument".to_string());
                                    }
                                    Box::new(ExpressionIndex::new(expression, v.remove(0)))
                                }
                            },
                        };
                        stack.push(ExpressionParserItem::SExpression(new_stack_item));
                    }
                    '{' => {
                        let v = Self::parse_member_list(lexer, '}')?;
                        stack.push(ExpressionParserItem::SExpression(Box::new(
                            ExpressionMap::new(v),
                        )));
                    }
                    _ => {
                        if stops.contains(br) {
                            stop = *br;
                            break;
                        } else {
                            return Result::Err(format!("Unexpected '{}'", br));
                        }
                    }
                },
                Token::Separator(sep) => {
                    if stops.contains(sep) {
                        stop = *sep;
                        break;
                    } else if *sep == '.' {
                        stack.push(ExpressionParserItem::SToken(Token::Separator('.')));
                    }
                }
                Token::ExpressionSeparator() => {
                    let expression = Self::stack_to_expression(&mut stack)?;
                    if !stack.is_empty() {
                        return Err("Failed to evaluate expression".to_string());
                    }
                    if let Some(e) = expression {
                        expressions.push(e);
                    }
                }
                Token::Error(err) => {
                    return Result::Err(err.clone());
                }
            }
        }
        if let Some(e) = Self::stack_to_expression(&mut stack)? {
            expressions.push(e);
        }
        if !stack.is_empty() {
            Err("Failed to evaluate expression".to_string())
        } else if expressions.is_empty() {
            Ok((stop, None))
        } else if expressions.len() == 1 {
            Ok((stop, expressions.pop()))
        } else {
            Ok((stop, Some(Box::new(ExpressionSequence::new(expressions)))))
        }
    }

    /// Removes both neighbours of the item at the index, then call the function with the
    /// neighbours and replace the item at the index with the result.\
    /// If the operation fails, all items (at index and neighbours) are removed.\
    /// Neighbours must be ExpressionParserItem::SExpression.
    fn fold_stack_at<F>(stack: &mut Vec<ExpressionParserItem>, idx: usize, f: F) -> bool
    where
        F: Fn(Box<dyn Expression>, Box<dyn Expression>) -> Result<Box<dyn Expression>, String>,
    {
        if idx > 0 && (idx + 1) < stack.len() {
            let right = stack.remove(idx + 1);
            stack.remove(idx);
            let left = stack.remove(idx - 1);

            if let ExpressionParserItem::SExpression(re) = right {
                if let ExpressionParserItem::SExpression(le) = left {
                    match f(le, re) {
                        Ok(expression) => {
                            stack.insert(idx - 1, ExpressionParserItem::SExpression(expression));
                            return true;
                        }
                        Err(_err) => return false,
                    }
                }
            }
        }
        false
    }

    /// Tries to create an expression from the current contents of the parser-stack.
    fn stack_to_expression(stack: &mut Vec<ExpressionParserItem>) -> Result<Option<Box<dyn Expression>>, String> {
        #[cfg(feature = "Debug")]
        debug!(
            "ExpressionParser.stack_to_expression: stack={:?}",
            vec_to_string(stack)
        );
        if stack.is_empty() {
            return Result::Ok(None);
        }
        // Handle operators and identifier
        let mut best_idx = 0usize;
        let mut best_idx_prio = 0xffu8;
        // Fold Methods on variables. Currently, this will not work with the logic below.
        let mut si = 0;
        while si < stack.len() {
            match &stack[si] {
                ExpressionParserItem::SToken(token) => match token {
                    Token::Identifier(identifier) => {
                        let ex = Box::new(ExpressionVariable::new(identifier));
                        stack[si] = ExpressionParserItem::SExpression(ex);
                    }
                    Token::Separator('.') => {
                        if 2 < best_idx_prio {
                            best_idx = si;
                            best_idx_prio = 2;
                        }
                    }
                    Token::Operator(operator) => {
                        let prio = match operator {
                            Operator::Not => 3u8,
                            Operator::And => 5,
                            Operator::Multiply => 5,
                            Operator::Divide => 5,
                            Operator::Modulus => 5,
                            Operator::Or => 6,
                            Operator::Plus => 6,
                            Operator::Minus => 6,
                            Operator::Less => 9,
                            Operator::LessEqual => 9,
                            Operator::Greater => 9,
                            Operator::GreaterEqual => 9,
                            Operator::Equal => 10,
                            Operator::NotEqual => 10,
                            Operator::Assign => 16,
                            Operator::AssignUndefined => 16,
                        };
                        if prio <= best_idx_prio {
                            best_idx = si;
                            best_idx_prio = prio;
                        }
                    }
                    _ => {
                        panic!("Internal error")
                    }
                },
                ExpressionParserItem::SExpression(_) => {}
            }
            si += 1;
        }
        if best_idx_prio < 0xffu8 {
            let mut op = None;
            let si = stack.get(best_idx).unwrap();
            if let ExpressionParserItem::SToken(Token::Operator(op_t)) = si {
                op = Some(op_t.clone());
            }
            if let Some(op) = op {
                match op.clone() {
                    Operator::Divide
                    | Operator::And
                    | Operator::Or
                    | Operator::Plus
                    | Operator::Minus
                    | Operator::Less
                    | Operator::LessEqual
                    | Operator::Greater
                    | Operator::GreaterEqual
                    | Operator::Equal
                    | Operator::NotEqual
                    | Operator::Modulus
                    | Operator::Multiply => {
                        if Self::fold_stack_at(
                            stack,
                            best_idx,
                            |le: Box<dyn Expression>, re: Box<dyn Expression>| -> Result<Box<dyn Expression>, String> {
                                Ok(Box::new(ExpressionOperator::new(op.clone(), le, re)))
                            },
                        ) {
                            return Self::stack_to_expression(stack);
                        }
                    }
                    Operator::AssignUndefined => {
                        if Self::fold_stack_at(
                            stack,
                            best_idx,
                            |le: Box<dyn Expression>, re: Box<dyn Expression>| -> Result<Box<dyn Expression>, String> {
                                Ok(Box::new(ExpressionAssignUndefined::new(le, re)))
                            },
                        ) {
                            return Self::stack_to_expression(stack);
                        }
                    }
                    Operator::Assign => {
                        if Self::fold_stack_at(
                            stack,
                            best_idx,
                            |le: Box<dyn Expression>, re: Box<dyn Expression>| -> Result<Box<dyn Expression>, String> {
                                Ok(Box::new(ExpressionAssign::new(le, re)))
                            },
                        ) {
                            return Self::stack_to_expression(stack);
                        }
                    }
                    Operator::Not => {
                        if (best_idx + 1) < stack.len() {
                            stack.remove(best_idx);
                            let right = stack.remove(best_idx);
                            if let ExpressionParserItem::SExpression(re) = right {
                                stack.insert(
                                    best_idx,
                                    ExpressionParserItem::SExpression(Box::new(ExpressionNot::new(re))),
                                );
                                return Self::stack_to_expression(stack);
                            }
                        }
                    }
                }
                return Err(format!("Failed to parse at operator '{:?}'", op));
            } else if let ExpressionParserItem::SToken(Token::Separator(sep_char)) = *si {
                if best_idx > 0
                    && (best_idx + 1) < stack.len()
                    && Self::fold_stack_at(
                        stack,
                        best_idx,
                        |le: Box<dyn Expression>, re: Box<dyn Expression>| -> Result<Box<dyn Expression>, String> {
                            if let Some(variable) = get_expression_as::<ExpressionVariable>(re.deref()) {
                                return Ok(Box::new(ExpressionMemberAccess::new(
                                    le,
                                    variable.name.clone(),
                                )));
                            }
                            if let Some(method) = get_expression_as::<ExpressionMethod>(re.deref()) {
                                let mut method_copy = method.get_copy();
                                method_copy.arguments.insert(0, le);
                                Ok(method_copy)
                            } else {
                                Err("No Field/Method on right side of '.'".to_string())
                            }
                        },
                    )
                {
                    return Self::stack_to_expression(stack);
                } else {
                    return Err(format!("Failed to parse at '{}'", sep_char));
                }
            }
        } else {
            let x = stack.remove(0);
            if let ExpressionParserItem::SExpression(ex) = x {
                // No operator? Return first one.
                return Ok(Some(ex));
            } else {
                return Err(format!("Failed to parse at '{}'", x));
            }
        }
        Err("Failed to parse".to_string())
    }
}

#[cfg(test)]
mod tests {
    use crate::datamodel::{create_data_arc, create_global_data_arc, Data};
    use crate::expression_engine::expressions::{get_expression_as, ExpressionIndex, ExpressionResult};
    use crate::expression_engine::parser::ExpressionParser;
    use std::collections::HashMap;
    use std::ops::Deref;

    #[test]
    fn parser_can_parse_a_simple_expression_without_identifiers() {
        let global_data = create_global_data_arc();

        let r = ExpressionParser::parse("12 * 3.4".to_string()).unwrap();
        print!("Parsed: {:?}", r);
        let result_data = r.execute(&mut global_data.lock().unwrap(), true);
        println!(" => {:?}", result_data);
        assert!(
            result_data.eq(&ExpressionResult::Ok(create_data_arc(Data::Double(
                12f64 * 3.4f64
            ))))
        );

        let r = ExpressionParser::parse("(12 * 2)".to_string()).unwrap();
        print!("Parsed: {:?}", r);
        let result_data = r.execute(&mut global_data.lock().unwrap(), true);
        println!(" => {:?}", result_data);
        assert!(result_data.eq(&ExpressionResult::Ok(create_data_arc(Data::Integer(24)))));

        let r = ExpressionParser::parse("(1 * 2) + (12 * 2)".to_string()).unwrap();
        print!("Parsed: {:?}", r);
        let result_data = r.execute(&mut global_data.lock().unwrap(), true);
        println!(" => {:?}", result_data);
        assert!(result_data.eq(&ExpressionResult::Ok(create_data_arc(Data::Integer(26)))));
    }

    #[test]
    fn expressions_prioritize_multiplication_division_operations() {
        let global_data = create_global_data_arc();

        let r = ExpressionParser::parse("12 + 2 * 4".to_string()).unwrap();
        print!("Parsed: {:?}", r);
        let result_data = r.execute(&mut global_data.lock().unwrap(), true);
        println!(" => {:?}", result_data);
        assert!(
            result_data.eq(&ExpressionResult::Ok(create_data_arc(Data::Integer(
                12 + (2 * 4)
            ))))
        );

        // Check that forced "()" work
        let r = ExpressionParser::parse("(12 + 2) * 4".to_string()).unwrap();
        print!("Parsed: {:?}", r);
        let result_data = r.execute(&mut global_data.lock().unwrap(), true);
        println!(" => {:?}", result_data);
        assert!(
            result_data.eq(&ExpressionResult::Ok(create_data_arc(Data::Integer(
                (12 + 2) * 4
            ))))
        );
    }

    #[test]
    fn can_parse_methods() {
        // let mut data = GlobalData::new();

        let r = ExpressionParser::parse("method(1,2,3,4)".to_string()).unwrap();
        println!("Parsed: {:?}", r);
    }

    #[test]
    fn can_parse_members() {
        let r1 = ExpressionParser::parse("A.b".to_string()).unwrap();
        println!("Parsed: {:?}", r1);

        let r2 = ExpressionParser::parse("A.b.c".to_string()).unwrap();
        println!("Parsed: {:?}", r2);

        let global_data = create_global_data_arc();
        let mut hs1 = HashMap::new();
        let mut hs2 = HashMap::new();
        hs2.insert(
            "c".to_string(),
            create_data_arc(Data::String("hello".to_string())),
        );
        hs1.insert("b".to_string(), create_data_arc(Data::Map(hs2)));

        global_data
            .lock()
            .unwrap()
            .data
            .set_undefined("A".to_string(), Data::Map(hs1));
        let rs1 = r1.execute(&mut global_data.lock().unwrap(), true);
        println!("==> {:?}", rs1);
        assert!(if let ExpressionResult::Ok(_x) = rs1 {
            true
        } else {
            false
        });

        let rs2 = r2.execute(&mut global_data.lock().unwrap(), true);
        println!("==> {:?}", rs2);
        assert_eq!(
            rs2,
            ExpressionResult::Ok(create_data_arc(Data::String("hello".to_string())))
        )
    }

    #[test]
    fn can_parse_assignment() {
        let r1 = ExpressionParser::parse("A=2*6".to_string()).unwrap();
        println!("Parsed: {:?}", r1);

        let global_data = create_global_data_arc();

        let rs1 = r1.execute(&mut global_data.lock().unwrap(), true);
        println!("==> {:?}", rs1);
        assert_eq!(
            rs1,
            ExpressionResult::Ok(create_data_arc(Data::Integer(12)))
        );
        assert_eq!(
            global_data
                .lock()
                .unwrap()
                .data
                .get(&"A".to_string())
                .unwrap()
                .lock()
                .unwrap()
                .deref(),
            &Data::Integer(12)
        );
    }

    #[test]
    fn can_parse_multiple_expressions() {
        let r1 = ExpressionParser::parse("X?=2;A=X*6".to_string()).unwrap();
        println!("Parsed: {:?}", r1);

        let global_data = create_global_data_arc();
        let rs1 = r1.execute(&mut global_data.lock().unwrap(), true);
        println!("==> {:?}", rs1);
        assert_eq!(
            rs1,
            ExpressionResult::Ok(create_data_arc(Data::Integer(12)))
        );
        assert_eq!(
            global_data
                .lock()
                .unwrap()
                .data
                .get(&"A".to_string())
                .unwrap()
                .lock()
                .unwrap()
                .deref(),
            &Data::Integer(12)
        );
    }

    #[test]
    fn can_parse_array_index() {
        let r = ExpressionParser::parse("[1,2,3,4][1]".to_string()).unwrap();
        println!("Parsed: {:?}", r);
        assert!(get_expression_as::<ExpressionIndex>(r.deref()).is_some());
    }
}
