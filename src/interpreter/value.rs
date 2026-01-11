// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2026 Viacheslav Shynkarenko

//! Runtime values for the MIR interpreter.

use std::collections::HashMap;
use std::fmt;

/// A runtime value in the interpreter.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum Value {
    /// Integer value (unified to i128 for simplicity).
    Int(i128),
    /// Floating-point value.
    Float(f64),
    /// Boolean value.
    Bool(bool),
    /// String value.
    String(String),
    /// Tuple of values.
    Tuple(Vec<Value>),
    /// Struct instance: (type name, field values).
    Struct(String, HashMap<String, Value>),
    /// Enum variant: (type name, variant name, optional associated value).
    Enum(String, String, Option<Box<Value>>),
    /// Array/list of values.
    Array(Vec<Value>),
    /// Map of key-value pairs.
    Map(HashMap<String, Value>),
    /// Unit/None value.
    #[default]
    None,
    /// Reference to another value (for future use).
    Ref(Box<Value>),
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Int(v) => write!(f, "{}", v),
            Value::Float(v) => write!(f, "{}", v),
            Value::Bool(v) => write!(f, "{}", v),
            Value::String(v) => write!(f, "\"{}\"", v),
            Value::Tuple(values) => {
                write!(f, "(")?;
                for (i, v) in values.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", v)?;
                }
                write!(f, ")")
            }
            Value::Struct(name, fields) => {
                write!(f, "{}(", name)?;
                for (i, (k, v)) in fields.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}: {}", k, v)?;
                }
                write!(f, ")")
            }
            Value::Enum(type_name, variant, value) => {
                write!(f, "{}.{}", type_name, variant)?;
                if let Some(v) = value {
                    write!(f, "({})", v)?;
                }
                Ok(())
            }
            Value::Array(values) => {
                write!(f, "[")?;
                for (i, v) in values.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", v)?;
                }
                write!(f, "]")
            }
            Value::Map(entries) => {
                write!(f, "{{")?;
                for (i, (k, v)) in entries.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}: {}", k, v)?;
                }
                write!(f, "}}")
            }
            Value::None => write!(f, "none"),
            Value::Ref(v) => write!(f, "&{}", v),
        }
    }
}

impl Value {
    /// Check if this value is truthy (for conditionals).
    pub fn is_truthy(&self) -> bool {
        match self {
            Value::Bool(v) => *v,
            Value::Int(v) => *v != 0,
            Value::Float(v) => *v != 0.0,
            Value::String(v) => !v.is_empty(),
            Value::Array(v) => !v.is_empty(),
            Value::None => false,
            _ => true,
        }
    }

    /// Convert value to integer if possible.
    pub fn as_int(&self) -> Option<i128> {
        match self {
            Value::Int(v) => Some(*v),
            Value::Float(v) => Some(*v as i128),
            Value::Bool(v) => Some(if *v { 1 } else { 0 }),
            _ => None,
        }
    }

    /// Convert value to float if possible.
    pub fn as_float(&self) -> Option<f64> {
        match self {
            Value::Float(v) => Some(*v),
            Value::Int(v) => Some(*v as f64),
            _ => None,
        }
    }

    /// Check if this is a numeric value.
    pub fn is_numeric(&self) -> bool {
        matches!(self, Value::Int(_) | Value::Float(_))
    }
    /// Get the type name of the value.
    pub fn type_name(&self) -> String {
        match self {
            Value::Int(_) => "integer".to_string(),
            Value::Float(_) => "float".to_string(),
            Value::Bool(_) => "boolean".to_string(),
            Value::String(_) => "string".to_string(),
            Value::Tuple(_) => "tuple".to_string(),
            Value::Struct(name, _) => name.clone(),
            Value::Enum(name, variant, _) => format!("{}::{}", name, variant),
            Value::Array(_) => "array".to_string(),
            Value::Map(_) => "map".to_string(),
            Value::None => "none".to_string(),
            Value::Ref(_) => "reference".to_string(),
        }
    }
}
