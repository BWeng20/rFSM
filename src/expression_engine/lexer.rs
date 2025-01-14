//! Implementation of a simple expression parser (lexer part).

use std::fmt;
use std::fmt::{Debug, Display, Formatter};

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
    And,
    Or,

    /// C-like modulus (mathematically the remainder) function.
    Modulus,
    Not,
}

/// Numeric types.
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

/// Token variants, generated by Lexer.
#[derive(PartialEq, Debug)]
pub enum Token {
    /// Some constant number. Integer or float.
    Number(NumericToken),
    /// An identifier
    Identifier(String),
    /// Some constant string expression
    TString(String),
    /// A constant boolean expression.
    Boolean(bool),
    /// Some operator
    Operator(Operator),
    /// Some bracket
    Bracket(char),
    /// A - none whitespace, none bracket - separator
    Separator(char),
    /// The expression separator to join multiple expressions.
    ExpressionSeparator(),
    /// a Null value
    Null(),
    /// Indicates a lexer error.
    Error(String),
    /// Indicates the end of the expression.
    EOE,
}

impl Display for Token {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        Debug::fmt(self, f)
    }
}

/// Lexer for Expressions. \
/// Generates tokens from text.
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
        Self::is_whitespace(c)
            || match c {
            '\0' | '.' | '!' | ',' | '\\' |
            // Operators
            '-' | '+' | '/' | ':' | '*' | '&' | '|' |
            '<' | '>' | '=' | '%' | '?' |
            // Brackets
            '[' | ']' | '(' | ')' | '{' | '}' |
            // String
            '"' | '\'' |
            // Expressions Separator
            ';'
            => { true }
            _ => { false }

        }
    }

    fn is_string_delimiter(c: char) -> bool {
        matches!(c, '\'' | '"')
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

    /// Read a String.\
    /// delimiter - The delimiter\
    /// Escape sequences see String state-chart on JSON.org.
    fn read_string(&mut self, delimiter: char) -> Token {
        let mut escape = false;
        let mut c;
        loop {
            c = self.next_char();
            if c == '\0' {
                return Token::Error("Missing string delimiter".to_string());
            } else if escape {
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
            } else if c == delimiter {
                return Token::TString(self.buffer.clone());
            }
            self.buffer.push(c);
        }
    }

    /// Read (possible combined) operators
    fn read_operator(&mut self, first: char) -> Token {
        Token::Operator(match first {
            '-' => Operator::Minus,
            '+' => Operator::Plus,
            '*' => Operator::Multiply,
            ':' | '/' => Operator::Divide,
            '&' => Operator::And,
            '|' => Operator::Or,
            '%' => Operator::Modulus,
            _ => {
                let second = self.next_char();
                if second == '=' {
                    match first {
                        '?' => Operator::AssignUndefined,
                        '<' => Operator::LessEqual,
                        '>' => Operator::GreaterEqual,
                        '=' => Operator::Equal,
                        '!' => Operator::NotEqual,
                        _ => {
                            // This method shall not be called with other chars.
                            return Token::Error("Internal Error".to_string());
                        }
                    }
                } else {
                    self.push_back();
                    match first {
                        '<' => Operator::Less,
                        '>' => Operator::Greater,
                        '=' => Operator::Assign,
                        '!' => Operator::Not,
                        _ => {
                            // This method shall not be called with other chars.
                            return Token::Error("Internal Error".to_string());
                        }
                    }
                }
            }
        })
    }

    /// Read a JSON Number (see state chart at JSON.org).
    /// c - The starting character.
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
                        return Token::Operator(Operator::Plus);
                    }
                    5 => {
                        self.push_back();
                        return Token::Operator(Operator::Minus);
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
                        return Token::Operator(Operator::Minus);
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
                        return Token::Operator(Operator::Minus);
                    }
                    _ => {
                        self.push_back();
                        break;
                    }
                }
            } else {
                if c != '\0' {
                    self.push_back();
                }
                break;
            }
            self.buffer.push(c);
            c = self.next_char();
        }
        match state {
            1 => {
                let r = self.buffer.parse::<i64>();
                match r {
                    Ok(v) => Token::Number(NumericToken::Integer(v)),
                    Err(err) => Token::Error(err.to_string()),
                }
            }
            2 | 4 => {
                if self.buffer.len() == 1 {
                    // Special case '.'
                    Token::Separator('.')
                } else {
                    let r = self.buffer.parse::<f64>();
                    match r {
                        Ok(v) => Token::Number(NumericToken::Double(v)),
                        Err(err) => Token::Error(err.to_string()),
                    }
                }
            }
            3 | 6 => Token::Error("missing exponent in number".to_string()),
            5 => Token::Operator(Operator::Minus),
            _ => Token::Error("internal error".to_string()),
        }
    }

    /// A much, much simpler replacement for char.is_digit(10).
    #[inline(always)]
    fn is_digit(c: char) -> bool {
        c.is_ascii_digit()
    }

    /// Check for a JSON whitespace.
    #[inline(always)]
    fn is_whitespace(c: char) -> bool {
        matches!(c, ' ' | '\n' | '\r' | '\t')
    }

    /// Parse and return the next token.
    pub fn next_token(&mut self) -> Token {
        self.next_token_with_stop(&[])
    }

    pub fn next_token_with_stop(&mut self, hard_stops: &[char]) -> Token {
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
                if self.buffer.is_empty() {
                    if Self::is_string_delimiter(c) {
                        // At start of string
                        return self.read_string(c);
                    } else if hard_stops.contains(&c) {
                        return Token::Separator(c);
                    } else {
                        // return the current stop as symbol
                        match c {
                            '\0' => {
                                return Token::EOE;
                            }
                            '?' | '+' | '-' | '*' | '<' | '>' | '=' | '%' | '/' | ':' | '!' | '&' | '|' => {
                                return self.read_operator(c);
                            }
                            '{' | '}' | '(' | ')' | '[' | ']' => {
                                return Token::Bracket(c);
                            }
                            ';' => {
                                return Token::ExpressionSeparator();
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

    /// Return the next token as a number, otherwise return Error.
    pub fn next_number(&mut self) -> Result<NumericToken, String> {
        let t = self.next_token();
        match t {
            Token::Number(e) => Ok(e),
            Token::Error(s) => Err(s),
            _ => Err("".to_string()),
        }
    }

    /// Return the next token to an Identifier, otherwise return Error.
    pub fn next_name(&mut self) -> Result<String, String> {
        let t = self.next_token();
        match t {
            Token::Identifier(e) => Ok(e),
            x => Err(format!("Unexpected token {}", x)),
        }
    }

    /// Checks if the lexer has at least one token remaining.
    pub fn has_next(&self) -> bool {
        self.pos < self.text.len()
    }

    /// Easts whitespaces.
    fn eat_space(&mut self) {
        while self.has_next() && Self::is_whitespace(self.text[self.pos]) {
            self.pos += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::expression_engine::lexer::{ExpressionLexer, NumericToken, Operator, Token};

    #[test]
    fn lexer_can_parse_numbers() {
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
        assert_eq!(n3, Token::Operator(Operator::Plus));

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
        assert_eq!(n9, Token::EOE);
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
        assert_eq!(n4, Token::EOE);
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
        assert_eq!(n4, Token::EOE);
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
        assert_eq!(n3, Token::EOE);
    }

    #[test]
    fn lexer_can_parse_operators() {
        let mut l = ExpressionLexer::new("|&=<>!+-*/:% <= >= != ==".to_string());

        let n = l.next_token();
        print!("{:?}", n);
        assert_eq!(n, Token::Operator(Operator::Or));

        let n = l.next_token();
        print!("{:?}", n);
        assert_eq!(n, Token::Operator(Operator::And));

        let n = l.next_token();
        print!("{:?}", n);
        assert_eq!(n, Token::Operator(Operator::Assign));

        let n = l.next_token();
        print!(" {:?}", n);
        assert_eq!(n, Token::Operator(Operator::Less));

        let n = l.next_token();
        print!(" {:?}", n);
        assert_eq!(n, Token::Operator(Operator::Greater));

        let n = l.next_token();
        print!(" {:?}", n);
        assert_eq!(n, Token::Operator(Operator::Not));

        let n = l.next_token();
        print!(" {:?}", n);
        assert_eq!(n, Token::Operator(Operator::Plus));

        let n = l.next_token();
        print!(" {:?}", n);
        assert_eq!(n, Token::Operator(Operator::Minus));

        let n = l.next_token();
        print!(" {:?}", n);
        assert_eq!(n, Token::Operator(Operator::Multiply));

        let n = l.next_token();
        print!(" {:?}", n);
        assert_eq!(n, Token::Operator(Operator::Divide));

        let n = l.next_token();
        print!(" {:?}", n);
        assert_eq!(n, Token::Operator(Operator::Divide));

        let n = l.next_token();
        print!(" {:?}", n);
        assert_eq!(n, Token::Operator(Operator::Modulus));

        let n = l.next_token();
        print!(" {:?}", n);
        assert_eq!(n, Token::Operator(Operator::LessEqual));

        let n = l.next_token();
        print!(" {:?}", n);
        assert_eq!(n, Token::Operator(Operator::GreaterEqual));

        let n = l.next_token();
        print!(" {:?}", n);
        assert_eq!(n, Token::Operator(Operator::NotEqual));

        let n = l.next_token();
        print!(" {:?}", n);
        assert_eq!(n, Token::Operator(Operator::Equal));

        let n = l.next_token();
        println!(" {:?}", n);
        assert_eq!(n, Token::EOE);
    }
}
