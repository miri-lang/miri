pub const USE_STATEMENT: &str = "
// Local module 
use Calc

// Global module
use System.Math

// Local module with path
use MyProject.Path.SomeModule

// Selective import from a module
use func1, func2 from Module1

// Local module with path and alias
use Module2 as M2
";

pub const INLINE_COMMENTS: &str = r#"
var x = 10 // simple inline comment

print 'Hello' // 👋 this is a friendly comment

use System.Math // use System.Math // with another comment inside

x = x + 1 // math: x becomes x + 1
"#;

pub const MULTILINE_COMMENTS: &str = r#"
/**/

/* This is a single-line comment */

/*****************************************/

/* This is a basic
multiline comment
spanning three lines */
some = "code"

/* Multiline comment with code inside:
var a = 5
print 'ignored!'
*/

func() int: 10 + 10

/***
/* 
  /* nested */ 
*/ 
***/

/*

  |\_/|
  ( o.o )   <- Cat!
  > ^ <

This is a comment with ASCII art.

Symbols: /* nested? */ < > & ^ ~
*/

print "Hello" /* inline comment */
"#;

pub const DECLARATION_STATEMENT: &str = "
x = 10                                   // inferred
var y = 20                               // mutable
z int = 30                               // explicitly typed
num = 5.0                                // float
str string = 'Hello'                     // string
is_active = true                         // boolean
even = 10 % 2 == 0                       // even number check
m = Map<string, int>()                   // map declaration
arr1 = [10, 20, 30]                      // array
arr2 [float] = [1.0, 2.0, 3.0]           // array with type
dict1 = {key1: 'A', key2: 'B'}           // dictionary
dict2 {string: int} = {key1: 1, key2: 2} // dictionary with type
";

pub const FUNCTION_STATEMENT: &str = "
// Function with no parameters
fancy_print():
  print \"Hello, World!\"

/* Function with parameters */
square(x int) int:
  x * x

/* Another function example */
add(a int, b int) int:
  a + b

// Inline function
multiply(a int, b int) int: a * b

// Lambda function
f = (x int) int: x * x

// Multiline lambda function
f1 = (a float, b float):
  print a + b
  print a - b

// Calls without parentheses
fancy_print
f 10
f1 5.0, 3.0

// Call with parentheses
fancy_print()
f(10)
f1(5.0, 3.0)

// Code block
y = arr.map:
  (x int) x * 2

// Nested function
nested_func(a int) int:
  inner_func(x int) int:
    print x
    res = x + 1
    for i in 0..x:
      print i
    print res
  inner_func(a)

nested_func(5)

";

pub const INDENT_DEDENT_FUNC: &str = "
// Normal call
func(10, \"hello\", 50)

// Indented call
func(10,
     \"hello\",
     50)

// Indented call with nested indentation
func(10,
     50,
     nested_func(x int) int:
       print x
       return x + 1)

// Indented call with all arguments on new lines
func(
  10,
  50
)
";

pub const INDENT_DEDENT_COMMENTS: &str = "
      // this is just a comment

// still a comment

  /*
    /* and this is another comment 
      */
*/


  // Comment 1
    // Comment 2
        // Comment 3
      // Comment 4
        // Comment 5
// Comment 6
";