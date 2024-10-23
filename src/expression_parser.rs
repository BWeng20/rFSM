//! Implementation of a simple expression parser.

use crate::datamodel::Data;
use crate::expressions::{ConstantExpression, Expression, ExpressionMethod, ExpressionVariable};
use std::fmt;
use std::fmt::{Debug, Display, Formatter};
use std::option::Option;

#[derive(PartialEq, Debug)]
pub enum NumericToken {
    Integer(i64),
    Double(f64),
}

impl NumericToken {
    pub fn as_double(&self) -> f64 {
        match self {
            NumericToken::Integer(i) => *i as f64,
            NumericToken::Double(d) => *d,
        }
    }
}

#[derive(PartialEq, Debug)]
pub enum OperatorToken {
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

#[derive(PartialEq, Debug)]
pub enum Token {
    Number(NumericToken),
    /// A identifier
    Identifier(String),
    /// Content of a constant string expression
    TString(String),
    Boolean(bool),
    Operator(OperatorToken),
    Bracket(char),
    /// A - none whitespace, none bracket - separator
    Separator(char),
    Null(),
    Error(String),
    EOF,
}

impl Display for Token {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        Debug::fmt(self, f)
    }
}

pub struct ExpressionLexer {
    text: Vec<char>,
    pos: usize,
    buffer: String,
}

impl ExpressionLexer {
    pub fn new(text: String) -> Self {
        let mut chars = Vec::with_capacity(text.len());
        for c in text.chars() {
            chars.push(c);
        }
        ExpressionLexer {
            text: chars,
            pos: 0,
            buffer: String::with_capacity(100),
        }
    }

    fn is_stop(c: char) -> bool {
        return Self::is_whitespace(c)
            || match c {
            '\0' | '.' | '!' | ',' | '\\' | ';' |
            // Operators
            '-' | '+' | '/' | ':' | '*' |
            '<' | '>' | '=' | '%' |
            // Brackets
            '(' | ')' | '{' | '}' |
            // String
            '"' | '\'' => { true }
            _ => { false }

        };
    }

    fn is_string_delimiter(c: char) -> bool {
        match c {
            '\'' | '"' => true,
            _ => false,
        }
    }

    pub fn next_char(&mut self) -> char {
        if self.pos < self.text.len() {
            let c = self.text[self.pos];
            self.pos += 1;
            c
        } else {
            '\0'
        }
    }

    pub fn push_back(&mut self) {
        if self.pos > 0 {
            self.pos -= 1;
        }
    }

    // Read a String.
    // delimiter - The delimiter
    // Escape sequences see String state-chart on JSON.org.
    fn read_string(&mut self, delimiter: char) -> Token {
        let mut escape = false;
        let mut c;
        loop {
            c = self.next_char();
            if c == delimiter || c == '\0' {
                // TODO: Shall we make '\0' an error?
                return Token::TString(self.buffer.clone());
            }
            if escape {
                match c {
                    '"' | '\\' | '/' => {}
                    'b' => {
                        c = '\x08';
                    }
                    'f' => {
                        c = '\x0c';
                    }
                    'n' => {
                        c = '\n';
                    }
                    'r' => {
                        c = '\r';
                    }
                    't' => {
                        c = '\t';
                    }
                    'u' => {
                        // 4 hex digits
                        let mut codepoint = String::with_capacity(4);
                        for _i in 0..4 {
                            let c = self.next_char();
                            if Self::is_digit(c) {
                                codepoint.push(c);
                            } else {
                                return Token::Error("Illegal \\u sequence in String".to_string());
                            }
                        }
                        let cp = u32::from_str_radix(codepoint.as_str(), 16);
                        match cp {
                            Ok(cpv) => match char::from_u32(cpv) {
                                None => {
                                    return Token::Error(format!(
                                        "Illegal codepoint {} in \\u sequence {}",
                                        cpv, codepoint
                                    ));
                                }
                                Some(cpc) => {
                                    c = cpc;
                                }
                            },
                            Err(_err) => {
                                return Token::Error(format!("Illegal \\u sequence {}", codepoint));
                            }
                        }
                    }
                    _ => {
                        return Token::Error("Illegal escape sequence in String".to_string());
                    }
                }
                escape = false;
            } else if c == '\\' {
                escape = true;
                continue;
            }
            self.buffer.push(c);
        }
    }

    /// Read possible combinaed operators
    fn read_operator(&mut self, first: char) -> Token {
        Token::Operator(match first {
            '-' => OperatorToken::Minus,
            '+' => OperatorToken::Plus,
            '*' => OperatorToken::Multiply,
            ':' | '/' => OperatorToken::Divide,
            '%' => OperatorToken::Modulo,
            _ => {
                let second = self.next_char();
                if second == '=' {
                    match first {
                        '<' => OperatorToken::LessEqual,
                        '>' => OperatorToken::GreaterEqual,
                        '=' => OperatorToken::Equal,
                        '!' => OperatorToken::NotEqual,
                        _ => {
                            // This method shall not be called with other chars.
                            return Token::Error("Internal Error".to_string());
                        }
                    }
                } else {
                    self.push_back();
                    match first {
                        '<' => OperatorToken::Less,
                        '>' => OperatorToken::Greater,
                        '=' => OperatorToken::Assign,
                        '!' => OperatorToken::Not,
                        _ => {
                            // This method shall not be called with other chars.
                            return Token::Error("Internal Error".to_string());
                        }
                    }
                }
            }
        })
    }

    // Read a JSON Number (see state chart at JSON.org).
    // c - The starting character.
    fn read_number(&mut self, mut c: char) -> Token {
        // States:
        // 0: Init
        // 1: In fix-point part
        // 2: In fraction part
        // 3: Just after "E"
        // 4: In exponent
        // 5: On starting "-"
        // 6: On "-" or "+" after "E"

        let mut state = 0u8;
        loop {
            if c == '.' {
                match state {
                    0 | 1 | 5 => {
                        state = 2u8;
                    }
                    _ => {
                        self.push_back();
                        break;
                    }
                }
            } else if Self::is_digit(c) {
                match state {
                    0 | 5 => {
                        state = 1u8;
                    }
                    3 | 6 => {
                        state = 4u8;
                    }
                    _ => {}
                }
            } else if c == '+' {
                // According to JSON only legal just after the "E".
                match state {
                    0 => {
                        return Token::Operator(OperatorToken::Plus);
                    }
                    5 => {
                        self.push_back();
                        return Token::Operator(OperatorToken::Minus);
                    }
                    3 => {
                        state = 6u8;
                    }
                    _ => {
                        self.push_back();
                        break;
                    }
                }
            } else if c == '-' {
                // According to JSON only legal at start or just after the "E".
                match state {
                    0 => {
                        state = 5u8;
                    }
                    3 => {
                        state = 6u8;
                    }
                    5 => {
                        self.push_back();
                        return Token::Operator(OperatorToken::Minus);
                    }
                    _ => {
                        self.push_back();
                        break;
                    }
                }
            } else if c == 'E' || c == 'e' {
                match state {
                    1 | 2 => {
                        state = 3;
                    }
                    5 => {
                        return Token::Operator(OperatorToken::Minus);
                    }
                    _ => {
                        self.push_back();
                        break;
                    }
                }
            } else {
                self.push_back();
                break;
            }
            self.buffer.push(c);
            c = self.next_char();
        }
        match state {
            1 => {
                let r = self.buffer.parse::<i64>();
                return match r {
                    Ok(v) => Token::Number(NumericToken::Integer(v)),
                    Err(err) => Token::Error(err.to_string()),
                };
            }
            2 | 4 => {
                if self.buffer.len() == 1 {
                    // Special case '.'
                    return Token::Separator('.');
                } else {
                    let r = self.buffer.parse::<f64>();
                    return match r {
                        Ok(v) => Token::Number(NumericToken::Double(v)),
                        Err(err) => Token::Error(err.to_string()),
                    };
                }
            }
            3 | 6 => {
                return Token::Error("missing exponent in number".to_string());
            }
            5 => {
                return Token::Operator(OperatorToken::Minus);
            }
            _ => {
                return Token::Error("internal error".to_string());
            }
        }
    }

    // A much, much simpler variant instead of char.is_digit(10).
    #[inline(always)]
    fn is_digit(c: char) -> bool {
        return c >= '0' && c <= '9';
    }

    // Check for a JSON whitespace.
    #[inline(always)]
    fn is_whitespace(c: char) -> bool {
        match c {
            ' ' | '\n' | '\r' | '\t' => true,
            _ => false,
        }
    }

    /// Return the next token.
    pub fn next_token(&mut self) -> Token {
        // at start of new symbol, eat all spaces
        self.eat_space();
        self.buffer.clear();
        let mut c = self.next_char();

        // Start chars for a legal Number ('+' and "." NOT in JSON):
        if Self::is_digit(c) || c == '-' || c == '+' || c == '.' {
            return self.read_number(c);
        }
        loop {
            if Self::is_stop(c) {
                if self.buffer.len() == 0 {
                    if Self::is_string_delimiter(c) {
                        // At start of string
                        return self.read_string(c);
                    } else {
                        // return the current stop as symbol
                        match c {
                            '\0' => {
                                return Token::EOF;
                            }
                            '+' | '-' | '*' | '<' | '>' | '=' | '%' | '/' | ':' | '!' => {
                                return self.read_operator(c);
                            }
                            '{' | '}' | '(' | ')' | '[' | ']' => {
                                return Token::Bracket(c);
                            }
                            _ => {
                                return Token::Separator(c);
                            }
                        }
                    }
                } else if c != '\0' {
                    // handle this the next call
                    self.push_back();
                }
                return match self.buffer.as_str() {
                    "true" => Token::Boolean(true),
                    "false" => Token::Boolean(false),
                    "null" => Token::Null(),
                    _ => Token::Identifier(self.buffer.clone()),
                };
            }
            // append until stop is found.
            self.buffer.push(c);
            c = self.next_char();
        }
    }

    // Force next to a number, otherwise return Error.
    pub fn next_number(&mut self) -> Result<NumericToken, String> {
        let t = self.next_token();
        match t {
            Token::Number(e) => Ok(e),
            Token::Error(s) => Err(s),
            _ => Err("".to_string()),
        }
    }

    // Force next to a Name, otherwise return Error.
    pub fn next_name(&mut self) -> Result<String, ()> {
        let t = self.next_token();
        match t {
            Token::Identifier(e) => Ok(e),
            _ => Err(()),
        }
    }

    pub fn has_next(&self) -> bool {
        self.pos < self.text.len()
    }

    fn eat_space(&mut self) {
        while self.has_next() && Self::is_whitespace(self.text[self.pos]) {
            self.pos += 1;
        }
    }
}

pub struct ExpressionParser {}

impl ExpressionParser {
    pub fn parse(text: String) -> Result<Box<dyn Expression>, String> {
        let mut lexer = ExpressionLexer::new(text);

        let mut methods: Vec<Box<ExpressionMethod>> = Vec::new();
        let mut previous_identifier: Option<String> = None;
        let mut sequence: Vec<Box<dyn Expression>> = Vec::new();
        loop {
            if previous_identifier.is_some() {
                match lexer.next_token() {
                    Token::Number(_) | Token::Identifier(_) | Token::TString(_) | Token::Boolean(_) | Token::Null() => {
                    }
                    Token::Operator(operator) => {}
                    Token::Bracket(br) => match br {
                        '(' => {}
                        ')' => {
                            let em = methods.pop();
                            match em {
                                None => {
                                    return Result::Err(format!("Unexpected {}", br));
                                }
                                Some(e) => {
                                    sequence.push(e);
                                }
                            }
                        }
                        _ => {
                            return Result::Err(format!("Unexpected {}", br));
                        }
                    },
                    Token::Separator(sep) => {}
                    Token::Error(err) => {
                        return Result::Err(err);
                    }
                    Token::EOF => {
                        if let Some(id) = previous_identifier {
                            sequence.push(Box::new(ExpressionVariable::new(id.as_str())));
                        }
                        break;
                    }
                }
            } else {
                match lexer.next_token() {
                    Token::Number(number) => match number {
                        NumericToken::Integer(v) => {
                            sequence.push(Box::new(ConstantExpression::new(Data::Integer(v))));
                        }
                        NumericToken::Double(v) => {
                            sequence.push(Box::new(ConstantExpression::new(Data::Double(v))));
                        }
                    },
                    Token::Identifier(identifier) => {
                        previous_identifier = Some(identifier.clone());
                    }
                    Token::TString(text) => {}
                    Token::Boolean(b) => {}
                    Token::Operator(operator) => {}
                    Token::Bracket(br) => {}
                    Token::Separator(sep) => {}
                    Token::Null() => {}
                    Token::Error(err) => {
                        return Result::Err(err);
                    }
                    Token::EOF => {
                        break;
                    }
                }
            }
        }
        todo!()
    }
}

#[cfg(test)]
mod tests {
    use crate::expression_parser::{ExpressionLexer, NumericToken, OperatorToken, Token};

    #[test]
    fn can_parse_numbers() {
        let mut l = ExpressionLexer::new("123 345.123 +456 -123 1e10 1.0e10 0x123".to_string());

        let n1 = l.next_number();
        println!("N1: {:?}", n1);
        assert!(n1.is_ok());
        assert_eq!(n1.unwrap().as_double(), 123f64);

        let n2 = l.next_number();
        println!("N2: {:?}", n2);
        assert!(n2.is_ok());
        assert_eq!(n2.unwrap().as_double(), 345.123f64);

        // Leading "+" are not allowed in JSON. We will get this as operator.
        let n3 = l.next_token();
        println!("N3: {:?}", n3);
        assert_eq!(n3, Token::Operator(OperatorToken::Plus));

        let n3b = l.next_token();
        println!("N3b: {:?}", n3b);
        assert_eq!(n3b, Token::Number(NumericToken::Integer(456)));

        let n4 = l.next_number();
        println!("N4: {:?}", n4);
        assert!(n4.is_ok());
        assert_eq!(n4.unwrap().as_double(), -123f64);

        let n5 = l.next_number();
        println!("N5: {:?}", n5);
        assert!(n5.is_ok());
        assert_eq!(n5.unwrap().as_double(), 1e10f64);

        let n6 = l.next_number();
        println!("N6: {:?}", n6);
        assert!(n6.is_ok());
        assert_eq!(n6.unwrap().as_double(), 1e10f64);

        // Sorry, no hex in json
        let n7 = l.next_token();
        println!("N7: {:?}", n7);
        assert!(matches!(n7, Token::Number(NumericToken::Integer(0))));

        let n8 = l.next_token();
        println!("N8: {:?}", n8);
        assert_eq!(n8, Token::Identifier("x123".to_string()));

        let n9 = l.next_token();
        println!("N9: {:?}", n9);
        assert_eq!(n9, Token::EOF);
    }

    #[test]
    fn lexer_can_parse_names() {
        let mut l = ExpressionLexer::new(" abc efg.xyz  . ZzZ".to_string());

        let n1 = l.next_name();
        println!("N1: {:?}", n1);
        assert!(n1.is_ok());
        assert_eq!(n1.unwrap(), "abc");

        let n2 = l.next_name();
        println!("N2: {:?}", n2);
        assert!(n2.is_ok());
        assert_eq!(n2.unwrap(), "efg");

        let n3 = l.next_token();
        println!("N3: {:?}", n3);
        if let Token::Separator(d) = n3 {
            assert_eq!(d, '.');
        } else {
            assert!(false);
        }

        let n4 = l.next_name();
        println!("N4: {:?}", n4);
        assert!(n4.is_ok());
        assert_eq!(n4.unwrap(), "xyz");

        let n5 = l.next_token();
        println!("N5: {:?}", n5);
        if let Token::Separator(d) = n5 {
            assert_eq!(d, '.');
        } else {
            assert!(false);
        }

        let n6 = l.next_name();
        println!("N6: {:?}", n6);
        assert!(n6.is_ok());
        assert_eq!(n6.unwrap(), "ZzZ");
    }

    #[test]
    fn lexer_can_parse_strings() {
        let mut l = ExpressionLexer::new(" \"123.2\" 'abc\\f' 'xx\\u0008xx'".to_string());

        let n1 = l.next_token();
        println!("N1: {:?}", n1);
        assert_eq!(n1, Token::TString("123.2".to_string()));

        let n2 = l.next_token();
        println!("N2: {:?}", n2);
        assert_eq!(n2, Token::TString("abc\x0c".to_string()));

        let n3 = l.next_token();
        println!("N3: {:?}", n3);
        assert_eq!(n3, Token::TString("xx\x08xx".to_string()));

        let n4 = l.next_token();
        println!("N4: {:?}", n4);
        assert_eq!(n4, Token::EOF);
    }

    #[test]
    fn lexer_can_parse_boolean() {
        let mut l = ExpressionLexer::new(" true false 'true' TRUE".to_string());

        let n1 = l.next_token();
        println!("N1: {:?}", n1);
        assert_eq!(n1, Token::Boolean(true));

        let n2 = l.next_token();
        println!("N2: {:?}", n2);
        assert_eq!(n2, Token::Boolean(false));

        // Check that a string "true" is still a string.
        let n3 = l.next_token();
        println!("N3: {:?}", n3);
        assert_eq!(n3, Token::TString("true".to_string()));

        // Check that a true is case sensitive.
        let n4 = l.next_token();
        println!("N4: {:?}", n4);
        assert_eq!(n4, Token::Identifier("TRUE".to_string()));

        let n4 = l.next_token();
        println!("N4: {:?}", n4);
        assert_eq!(n4, Token::EOF);
    }

    #[test]
    fn lexer_can_parse_null() {
        let mut l = ExpressionLexer::new(" null 'null'".to_string());

        // Check that null is parsed.
        let n1 = l.next_token();
        println!("N1: {:?}", n1);
        assert_eq!(n1, Token::Null());

        // Check that "null" as string is still a string.
        let n2 = l.next_token();
        println!("N2: {:?}", n2);
        assert_eq!(n2, Token::TString("null".to_string()));

        let n3 = l.next_token();
        println!("N3: {:?}", n3);
        assert_eq!(n3, Token::EOF);
    }

    #[test]
    fn lexer_can_parse_operators() {
        let mut l = ExpressionLexer::new("=<>!+-*/:% <= >= != ==".to_string());

        let n = l.next_token();
        print!("{:?}", n);
        assert_eq!(n, Token::Operator(OperatorToken::Assign));

        let n = l.next_token();
        print!(" {:?}", n);
        assert_eq!(n, Token::Operator(OperatorToken::Less));

        let n = l.next_token();
        print!(" {:?}", n);
        assert_eq!(n, Token::Operator(OperatorToken::Greater));

        let n = l.next_token();
        print!(" {:?}", n);
        assert_eq!(n, Token::Operator(OperatorToken::Not));

        let n = l.next_token();
        print!(" {:?}", n);
        assert_eq!(n, Token::Operator(OperatorToken::Plus));

        let n = l.next_token();
        print!(" {:?}", n);
        assert_eq!(n, Token::Operator(OperatorToken::Minus));

        let n = l.next_token();
        print!(" {:?}", n);
        assert_eq!(n, Token::Operator(OperatorToken::Multiply));

        let n = l.next_token();
        print!(" {:?}", n);
        assert_eq!(n, Token::Operator(OperatorToken::Divide));

        let n = l.next_token();
        print!(" {:?}", n);
        assert_eq!(n, Token::Operator(OperatorToken::Divide));

        let n = l.next_token();
        print!(" {:?}", n);
        assert_eq!(n, Token::Operator(OperatorToken::Modulo));

        let n = l.next_token();
        print!(" {:?}", n);
        assert_eq!(n, Token::Operator(OperatorToken::LessEqual));

        let n = l.next_token();
        print!(" {:?}", n);
        assert_eq!(n, Token::Operator(OperatorToken::GreaterEqual));

        let n = l.next_token();
        print!(" {:?}", n);
        assert_eq!(n, Token::Operator(OperatorToken::NotEqual));

        let n = l.next_token();
        print!(" {:?}", n);
        assert_eq!(n, Token::Operator(OperatorToken::Equal));

        let n = l.next_token();
        println!(" {:?}", n);
        assert_eq!(n, Token::EOF);
    }
}
