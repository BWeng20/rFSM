//! Implementation of a simple expression parser.

use std::fmt;
use std::fmt::{Debug, Display, Formatter};
use crate::expression_parser::Token::{Number, Operator, Separator, TString};

#[derive(PartialEq,Debug)]
pub enum NumericToken {
    Integer(i64),
    Double(f64),
}

impl NumericToken {
    pub fn as_double(&self) -> f64 {
        match self {
            NumericToken::Integer(i) => { *i as f64 }
            NumericToken::Double(d) => { *d }
        }
    }
}

#[derive(PartialEq, Debug)]
pub enum OperatorToken {
    Multiply,
    Divide,
    Plus,
    Minus
}

#[derive(PartialEq, Debug)]
pub enum Token {
    Number(NumericToken),
    Name(String),
    TString(String),
    Boolean(bool),
    Operator(OperatorToken),
    Bracket(char),
    Separator(char)
}

impl Display for Token {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        Debug::fmt(self, f)
    }
}



pub struct ExpressionLexer {

    text : Vec::<char>,
    pos: usize

}

impl ExpressionLexer {

    pub fn new(text : String) -> Self{
        let mut chars = Vec::with_capacity(text.len());
        for c in text.chars() {
            chars.push(c);
        }
        ExpressionLexer {
            text: chars,
            pos: 0,
        }
    }

    pub fn next(&mut self) -> Option<Token> {
        self.eat_space();
        if self.has_next() {
            let mut numeric = true;
            let start_pos = self.pos;
            let mut separator = -1;

            let mut s = String::with_capacity(20);
            loop {
                let c = self.text[self.pos];
                self.pos += 1;

                if c.is_whitespace() {
                    break;
                } else if c.is_digit(10) {
                } else {
                    match c {
                        '.' => {
                            if !numeric {
                                self.pos -= 1;
                                break;
                            }
                            separator = self.pos as i32;
                        }
                        ',' => {
                            if s.is_empty() {
                                return Some(Separator(c));
                            }
                            self.pos -= 1;
                            break;
                        }
                        '*' => {
                            if s.is_empty() {
                                return Some(Operator(OperatorToken::Multiply));
                            }
                            self.pos -= 1;
                            break;
                        }
                        '+' => {
                            if !(s.is_empty() || numeric) {
                                self.pos -= 1;
                                break;
                            }
                            separator = self.pos as i32;
                        }
                        ':' => {
                            if s.is_empty() {
                                return Some(Operator(OperatorToken::Divide));
                            }
                            self.pos -= 1;
                            break;
                        }
                        '-' => {
                            if !(s.is_empty() || numeric) {
                                self.pos -= 1;
                                break;
                            }
                            separator = self.pos as i32;
                        }
                        _ => {
                            if separator > 0 {
                                // We had a separator character that could be part of a number.
                                // Now we know this was wrong.
                                self.pos -= 1;
                                if (self.pos - start_pos) == 1  {
                                    // Just the separator
                                    return Some(Token::Separator(self.text[start_pos]));
                                }
                                if numeric {
                                    // Keep the separator and handle as number
                                    break;
                                } else {
                                    let b = &self.text[start_pos..separator as usize];
                                    return Some(Token::Name(b.iter().collect()));
                                }
                            }
                            numeric = false;
                        }
                    }
                }
                s.push(c);
                if self.pos >= self.text.len() {
                    break;
                }
            }
            if numeric {
                if s.contains('.') {
                        match s.parse::<f64>() {
                            Ok(v) => {
                                return Some(Number(NumericToken::Double(v)));
                            }
                            Err(_) => {}
                        }
                } else {
                    match s.parse::<i64>() {
                        Ok(v) => {
                            return Some(Number(NumericToken::Integer(v)));
                        }
                        Err(_) => {
                        }
                    }
                }
            }
            if s.len() == 1 {
                // Check if we got some special.
                let c = s.as_bytes()[0] as char;
                if  c == '+' {
                    return Some(Operator(OperatorToken::Plus));
                }
                if  c == '-' {
                    return Some(Operator(OperatorToken::Minus));
                }
                if  c == '.' {
                    return Some(Separator(c));
                }
            }
            return Some(Token::Name(s));
        } else {
            None
        }
    }

    pub fn next_number(&mut self) -> Result<NumericToken,()> {
        if let Some(t) = self.next() {
            match t {
                Number(e) => {
                    Ok(e)
                }
                _ => {
                    Err(())
                }
            }
        } else {
            Err(())
        }
    }

    pub fn next_name(&mut self) -> Result<String,()> {
        if let Some(t) = self.next() {
            match t {
                Token::Name(e) => {
                    Ok(e)
                }
                _ => {
                    Err(())
                }
            }
        } else {
            Err(())
        }
    }

    pub fn has_next(&self) -> bool {
        self.pos < self.text.len()
    }

    fn eat_space(&mut self) {
        while self.has_next() && self.text[self.pos].is_whitespace() {
            self.pos += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::expression_parser::{ExpressionLexer, Token};

    #[test]
    fn can_parse_numbers() {

        let mut l = ExpressionLexer::new("123 345.123 +123 -123 +123.123".to_string());

        let n1 = l.next_number();
        println!("N1: {:?}", n1);
        assert!( n1.is_ok() );
        assert_eq!(n1.unwrap().as_double(), 123f64);

        let n2 = l.next_number();
        println!("N2: {:?}", n2);
        assert!( n2.is_ok() );
        assert_eq!(n2.unwrap().as_double(), 345.123f64);

        let n3 = l.next_number();
        println!("N3: {:?}", n3);
        assert!( n3.is_ok() );
        assert_eq!(n3.unwrap().as_double(), 123f64);

        let n4 = l.next_number();
        println!("N4: {:?}", n4);
        assert!( n4.is_ok() );
        assert_eq!(n4.unwrap().as_double(), -123f64);

        let n5 = l.next_number();
        println!("N5: {:?}", n5);
        assert!( n5.is_ok() );
        assert_eq!(n5.unwrap().as_double(), 123.123f64);
    }

    #[test]
    fn can_parse_strings() {

        let mut l = ExpressionLexer::new(" abc efg.xyz  . ZzZ".to_string());

        let n1 = l.next_name();
        println!("N1: {:?}", n1);
        assert!( n1.is_ok() );
        assert_eq!(n1.unwrap(), "abc");

        let n2 = l.next_name();
        println!("N2: {:?}", n2);
        assert!( n2.is_ok() );
        assert_eq!(n2.unwrap(), "efg");

        let n3 = l.next();
        println!("N3: {:?}", n3);
        assert!( n3.is_some() );
        if let Token::Separator(d) = n3.unwrap() {
            assert_eq!( d, '.' );
        }  else {
            assert!(false);
        }

        let n4 = l.next_name();
        println!("N4: {:?}", n4);
        assert!( n4.is_ok() );
        assert_eq!(n4.unwrap(), "xyz");

        let n5 = l.next();
        println!("N5: {:?}", n5);
        assert!( n5.is_some() );
        if let Token::Separator(d) = n5.unwrap() {
            assert_eq!( d, '.' );
        }  else {
            assert!(false);
        }

        let n6 = l.next_name();
        println!("N6: {:?}", n6);
        assert!( n6.is_ok() );
        assert_eq!(n6.unwrap(), "ZzZ");

    }

    #[test]
    fn can_parse_number_and_strings_with_delimiter() {
        let mut l = ExpressionLexer::new(" -123.xXx".to_string());

        let n1 = l.next_number();
        println!("N1: {:?}", n1);
        assert!( n1.is_ok() );
        assert_eq!(n1.unwrap().as_double(), -123f64 );

        let n2 = l.next_name();
        println!("N2: {:?}", n2);
        assert!( n2.is_ok() );
        assert_eq!(n2.unwrap(), "xXx");

    }
}