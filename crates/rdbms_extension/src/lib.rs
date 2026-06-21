//! Static extension registry v0.
//!
//! Stage 9 deliberately implements only the safe static path. Native dynamic
//! loading is still a documented ABI direction, not a runtime feature.

use rdbms_core::{DbError, DbResult, Value};
use rdbms_ext_abi::{abi_version_supported, RDBMS_EXT_ABI_VERSION};
use std::collections::BTreeMap;

/// Extension loading kind supported by Stage 9.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExtensionKind {
    /// Extension is linked into the process and registered by name.
    Static,
}

impl ExtensionKind {
    /// String persisted in catalog extension metadata.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Static => "static",
        }
    }
}

/// Function arity accepted by a scalar function.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScalarArity {
    /// Exactly N arguments.
    Exact(usize),
}

impl ScalarArity {
    fn accepts(self, len: usize) -> bool {
        match self {
            Self::Exact(expected) => expected == len,
        }
    }

    fn describe(self) -> String {
        match self {
            Self::Exact(expected) => expected.to_string(),
        }
    }
}

/// Scalar function descriptor.
#[derive(Clone, Copy)]
pub struct ScalarFunction {
    /// SQL-visible function name.
    pub name: &'static str,
    /// Accepted arity.
    pub arity: ScalarArity,
    /// Informational return type name.
    pub return_type: &'static str,
    /// Function implementation.
    pub eval: fn(&[Value]) -> DbResult<Value>,
}

/// Static extension descriptor.
#[derive(Clone, Copy)]
pub struct ExtensionDescriptor {
    /// Extension name.
    pub name: &'static str,
    /// ABI version expected by this extension.
    pub abi_version: u32,
    /// Loading kind.
    pub kind: ExtensionKind,
    /// Scalar functions exported by this extension.
    pub scalar_functions: &'static [ScalarFunction],
}

/// Runtime registry of loaded static extension functions.
#[derive(Clone, Default)]
pub struct ExtensionRegistry {
    extensions: BTreeMap<String, ExtensionDescriptor>,
    scalar_functions: BTreeMap<String, ScalarFunction>,
}

impl ExtensionRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Load a built-in static extension by name.
    pub fn load_static(&mut self, name: &str) -> DbResult<ExtensionDescriptor> {
        let descriptor = builtin_extension_descriptor(name)?;
        self.load(descriptor)?;
        Ok(descriptor)
    }

    /// Load a static descriptor after validating its ABI version.
    pub fn load(&mut self, descriptor: ExtensionDescriptor) -> DbResult<()> {
        if !abi_version_supported(descriptor.abi_version) {
            return Err(DbError::Extension(format!(
                "extension '{}' requires unsupported ABI version {}",
                descriptor.name, descriptor.abi_version
            )));
        }

        let normalized_name = normalize_name(descriptor.name);
        if self.extensions.contains_key(&normalized_name) {
            return Ok(());
        }

        for function in descriptor.scalar_functions {
            let function_name = normalize_name(function.name);
            if self.scalar_functions.contains_key(&function_name) {
                return Err(DbError::Extension(format!(
                    "scalar function already registered: {}",
                    function.name
                )));
            }
        }

        for function in descriptor.scalar_functions {
            self.scalar_functions
                .insert(normalize_name(function.name), *function);
        }
        self.extensions.insert(normalized_name, descriptor);
        Ok(())
    }

    /// Return true when an extension is loaded in this registry.
    pub fn is_loaded(&self, name: &str) -> bool {
        self.extensions.contains_key(&normalize_name(name))
    }

    /// Return a scalar function descriptor by SQL-visible name.
    pub fn scalar_function(&self, name: &str) -> Option<&ScalarFunction> {
        self.scalar_functions.get(&normalize_name(name))
    }

    /// Call a registered scalar function.
    pub fn call_scalar(&self, name: &str, args: &[Value]) -> DbResult<Value> {
        let function = self.scalar_function(name).ok_or(DbError::User(format!(
            "unknown scalar function: {name}"
        )))?;
        if !function.arity.accepts(args.len()) {
            return Err(DbError::User(format!(
                "scalar function '{}' expects {} argument(s), got {}",
                function.name,
                function.arity.describe(),
                args.len()
            )));
        }
        (function.eval)(args)
    }
}

/// Lookup a built-in extension descriptor by name.
pub fn builtin_extension_descriptor(name: &str) -> DbResult<ExtensionDescriptor> {
    match normalize_name(name).as_str() {
        "stdlib" => Ok(STDLIB_EXTENSION),
        other => Err(DbError::Extension(format!(
            "unknown static extension: {other}"
        ))),
    }
}

/// Names of built-in static extensions known to this build.
pub fn builtin_extension_names() -> &'static [&'static str] {
    &["stdlib"]
}

/// Build a registry from persisted catalog extension names.
pub fn registry_from_installed_extensions<'a, I>(installed: I) -> DbResult<ExtensionRegistry>
where
    I: IntoIterator<Item = (&'a str, u32, &'a str)>,
{
    let mut registry = ExtensionRegistry::new();
    for (name, abi_version, kind) in installed {
        if kind != ExtensionKind::Static.as_str() {
            return Err(DbError::Extension(format!(
                "unsupported extension kind for '{}': {}",
                name, kind
            )));
        }
        let descriptor = builtin_extension_descriptor(name)?;
        if descriptor.abi_version != abi_version {
            return Err(DbError::Extension(format!(
                "installed extension '{}' ABI mismatch: catalog={}, runtime={}",
                name, abi_version, descriptor.abi_version
            )));
        }
        registry.load(descriptor)?;
    }
    Ok(registry)
}

/// Name normalization shared by registry and SQL parser.
pub fn normalize_name(name: &str) -> String {
    name.to_ascii_lowercase()
}

static STDLIB_FUNCTIONS: &[ScalarFunction] = &[
    ScalarFunction {
        name: "length",
        arity: ScalarArity::Exact(1),
        return_type: "INT",
        eval: length,
    },
    ScalarFunction {
        name: "lower",
        arity: ScalarArity::Exact(1),
        return_type: "TEXT",
        eval: lower,
    },
    ScalarFunction {
        name: "upper",
        arity: ScalarArity::Exact(1),
        return_type: "TEXT",
        eval: upper,
    },
    ScalarFunction {
        name: "abs",
        arity: ScalarArity::Exact(1),
        return_type: "DYNAMIC",
        eval: abs_value,
    },
    ScalarFunction {
        name: "typeof",
        arity: ScalarArity::Exact(1),
        return_type: "TEXT",
        eval: typeof_value,
    },
    ScalarFunction {
        name: "rdbms_version",
        arity: ScalarArity::Exact(0),
        return_type: "TEXT",
        eval: rdbms_version,
    },
];

static STDLIB_EXTENSION: ExtensionDescriptor = ExtensionDescriptor {
    name: "stdlib",
    abi_version: RDBMS_EXT_ABI_VERSION,
    kind: ExtensionKind::Static,
    scalar_functions: STDLIB_FUNCTIONS,
};

fn length(args: &[Value]) -> DbResult<Value> {
    match &args[0] {
        Value::Null => Ok(Value::Null),
        Value::Text(value) => i64::try_from(value.chars().count())
            .map(Value::Integer)
            .map_err(|_| DbError::User("text length does not fit into INT".to_string())),
        _ => Err(DbError::User("length() expects TEXT".to_string())),
    }
}

fn lower(args: &[Value]) -> DbResult<Value> {
    match &args[0] {
        Value::Null => Ok(Value::Null),
        Value::Text(value) => Ok(Value::Text(value.to_ascii_lowercase())),
        _ => Err(DbError::User("lower() expects TEXT".to_string())),
    }
}

fn upper(args: &[Value]) -> DbResult<Value> {
    match &args[0] {
        Value::Null => Ok(Value::Null),
        Value::Text(value) => Ok(Value::Text(value.to_ascii_uppercase())),
        _ => Err(DbError::User("upper() expects TEXT".to_string())),
    }
}

fn abs_value(args: &[Value]) -> DbResult<Value> {
    match &args[0] {
        Value::Null => Ok(Value::Null),
        Value::Integer(value) => value
            .checked_abs()
            .map(Value::Integer)
            .ok_or(DbError::User("abs(INT) overflow".to_string())),
        Value::Double(value) => Ok(Value::Double(value.abs())),
        _ => Err(DbError::User("abs() expects INT or DOUBLE".to_string())),
    }
}

fn typeof_value(args: &[Value]) -> DbResult<Value> {
    let name = match &args[0] {
        Value::Null => "NULL",
        Value::Integer(_) => "INT",
        Value::Text(_) => "TEXT",
        Value::Double(_) => "DOUBLE",
    };
    Ok(Value::Text(name.to_string()))
}

fn rdbms_version(args: &[Value]) -> DbResult<Value> {
    if !args.is_empty() {
        return Err(DbError::InternalInvariant("arity check did not run"));
    }
    Ok(Value::Text("rdbms-stage9-extension-v0".to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_stdlib_and_calls_scalar_functions() -> DbResult<()> {
        let mut registry = ExtensionRegistry::new();
        let descriptor = registry.load_static("stdlib")?;

        assert_eq!(descriptor.abi_version, RDBMS_EXT_ABI_VERSION);
        assert!(registry.is_loaded("STDLIB"));
        assert_eq!(
            registry.call_scalar("upper", &[Value::Text("ada".to_string())])?,
            Value::Text("ADA".to_string())
        );
        assert_eq!(
            registry.call_scalar("length", &[Value::Text("abc".to_string())])?,
            Value::Integer(3)
        );
        Ok(())
    }

    #[test]
    fn rejects_unknown_function() -> DbResult<()> {
        let registry = ExtensionRegistry::new();
        let error = registry
            .call_scalar("missing", &[])
            .err()
            .ok_or(DbError::InternalInvariant("missing function was accepted"))?;
        assert!(matches!(error, DbError::User(_)));
        Ok(())
    }
}
