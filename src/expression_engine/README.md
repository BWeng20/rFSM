# Expression Datamodel

This module implements a fast and simple W3C-SCXML Datamodel.
It an expression-like, non-Turing-complete language. 

It is available if feature _"RfsmExpressionModel"_ is turned on.

### Selection of Datamodel

To select this model in SCXML use `datamodel="rfsm-expression"`. 

### Syntax (_DRAFT_)

```
  <expression-list>  ::= <expression> {";" <expression>}
  <expression>       ::= <sub-expression> [<operator> <expression>]
  <sub-expression>   ::= {"!"}<data>{ "." <method> | "." <identifier> | <index-expression> }
  <data>             ::= <method> | <identifier> | <constant>
  <method>           ::= <identifier> "(" <arguments> ")"
  <index-expression> ::= "[" <sub-expression> "]"
  <constant>         ::= "null" | <array> | <boolean> | <string> | <number>
  <array>            ::= "[" <arguments> "]"
  <boolean>          ::= "true" | "false"
  
  <number>           ::= <integer> <fraction> <exponent>
  <integer>          ::= ["-"]( <digit_wo_zero>{<digit>} | <digit> ) 
  <fraction>         ::= "" | "."<digit>{<digit>}
  <exponent>         ::= "" | ("E"|"e")[+|-]<digit>{<digit>}
  <digit_wo_zero>    ::= "1" .. "9"  
  <digit>            ::= "0" | <digit_wo_zero>  

  <string>           ::= '"' { <character> } '"' | "'" { <character> } "'"
  <character>        ::= As specified in JSON: unicode character. '"', "'", '\' and control characters escaped. 
  <arguments>        ::= <sub-expression>{"," <sub-expression>}
  <identifier>       ::= <letter>{<letter>|<digit>}
  <operator>         ::= "?=" | "=" | "==" | ">=" | "<=" | "*" | "%" | "+" | "-" | ":" | "/" | "&" | "|"
  <letter>           ::= "A" .. "Z" | "a" .. "z" | "_"  
```

Numbers are represented as specified in JSON.

The expressions will be authored as part of the SCXML document, here the encoding of the XML document is used.
The XML parser will convert all text into the encoding of the RUST-runtime. We expect RUST will stick to utf-8,
so the effective structure of a string in the runtime will be utf-8-encoded.<br/>
The structure of such strings is therefore not always identical to the XML source. 
In particular, the length and the individual characters can be different. 
Keep this in mind when performing string operations.   

### Operators

The available operators and their meaning

| Operator             | Name           | Description                                                                                                          |
|----------------------|----------------|----------------------------------------------------------------------------------------------------------------------|
| `=`                  | Assignment     | The result of the right side is assigned to the left side. Left side must specify an existing writable variable.     |
| `?=`                 | Initialisation | The left side is created and initialized with the result of the right side. Left side specifies a writable variable. |                                                 |
| `==`                 | Equal          | Results to `true` if the left side is equal to the right side.                                                       |
| `!=`                 | Not Equal      | Results to `true` if the left side is _not_ equal to the right side.                                                 |
| `>=`, `<=`, `>`, `<` | Comparison     | Results to `true` if left and right satisfies the condition.                                                         |
| `/`, `:`             | Division       | Works only on numeric types. Returns a Data::Double.                                                                 |
| `*`                  | Multiplication | Works only on numeric types. Returns a Data::Double if at least one operant is Double, otherwise Data::Integer.      |
| `+`                  | Aggregation    | Computes the sum for Data::Integer or Data::Double and the aggregation for Data::Map and Data::Array.                |
| `-`                  | Minus          | Computes the difference of left and right. Works only on numeric types.                                              |
| `%`                  | Modulus        | Computes the remainder of dividing left by right. Works only on numeric types.                                       |

SCXML requires that only declared variables can be written. An `=` to an undefined variable will return an error.
Nevertheless, it should  be possible to declare variables in the global &lt;script&gt; element.<br/>
In the ECMA-datamodel (in which the ECMA-interpreter is executed in strict mode) this is done via a _"var"_ declaration. <br/>
This expression language is not a script languages and thus has no such declaration syntax. Instead, you can use the "Initialisation" assignment operator `?=` 
to create and initialize a variable.<br/>

```
  myVar ?= [1,2,3,4]
```

### Custom Actions

Custom actions via the trait "Action" can be called like methods.

_Call them like global functions_

```
  length("a string")
```

_Call them like member-functions_<br/>
In this case, the value on which this action is called is given as first argument.
This works for all actions with at least one argument.

```
  "a string".length()
```

There are several pre-defined Actions:

| Action    | Arguments                                                                                                                                                                                          | Return value  | Description                                                                                    |
|-----------|----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|---------------|------------------------------------------------------------------------------------------------|
| abs       | One argument of type <ul><li>Data::Double</li><li>Data::Integer</li></ul>                                                                                                                          | Same as input | Computes the absolute value.                                                                   |
| length    | One argument of type <ul><li>Data::String - number of characters</li><li>Data::Array - number of items</li><li>Data::Map - number of entries</li><li>Data::Source - number of characters</li></ul> | Data::Integer | Get the length of the argument.                                                                |
| isDefined | One argument of any kind.                                                                                                                                                                          | Data::Boolean | Technical, this checks if the argument is not `Data::Error` or `Data::None`.                   |
| indexOf   | Two arguments of type Data::String.                                                                                                                                                                | Data::Integer | Get the index of the second string inside the first one. Returns -1, if the string not found.  |
| In        | One argument of type Data::String.                                                                                                                                                                 | Data::Boolean | Implements SCXML "In" function. Checks if the given state is inside the current configuration. |