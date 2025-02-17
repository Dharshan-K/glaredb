use std::collections::HashMap;
use std::fmt::Display;
use std::sync::Arc;

use crate::errors::{ExtensionError, Result};
use crate::vars::SessionVars;
use async_trait::async_trait;
use datafusion::arrow::datatypes::Fields;
use datafusion::datasource::TableProvider;
use datafusion::execution::context::SessionState;
use decimal::Decimal128;
use protogen::metastore::types::catalog::{CredentialsEntry, DatabaseEntry, RuntimePreference};
use protogen::rpcsrv::types::func_param_value::{
    FuncParamValue as ProtoFuncParamValue, FuncParamValueArrayVariant,
    FuncParamValueEnum as ProtoFuncParamValueEnum,
};

#[async_trait]
pub trait TableFunc: Sync + Send {
    /// The name for this table function. This name will be used when looking up
    /// function implementations.
    fn name(&self) -> &str;
    fn runtime_preference(&self) -> RuntimePreference;
    fn detect_runtime(
        &self,
        _args: &[FuncParamValue],
        _parent: RuntimePreference,
    ) -> Result<RuntimePreference> {
        Ok(self.runtime_preference())
    }

    /// Return a table provider using the provided args.
    async fn create_provider(
        &self,
        ctx: &dyn TableFuncContextProvider,
        args: Vec<FuncParamValue>,
        opts: HashMap<String, FuncParamValue>,
    ) -> Result<Arc<dyn TableProvider>>;
}
pub trait TableFuncContextProvider: Sync + Send {
    fn get_database_entry(&self, name: &str) -> Option<&DatabaseEntry>;
    fn get_credentials_entry(&self, name: &str) -> Option<&CredentialsEntry>;
    fn get_session_vars(&self) -> SessionVars;
    fn get_session_state(&self) -> SessionState;
    fn get_catalog_lister(&self) -> Box<dyn VirtualLister>;
}

#[async_trait]
pub trait VirtualLister: Sync + Send {
    /// List schemas for a data source.
    async fn list_schemas(&self) -> Result<Vec<String>>;

    /// List tables for a data source.
    async fn list_tables(&self, schema: &str) -> Result<Vec<String>>;

    /// List columns for a specific table in the datasource.
    async fn list_columns(&self, schema: &str, table: &str) -> Result<Fields>;
}

use std::fmt;

use datafusion::scalar::ScalarValue;

/// Value from a function parameter.
#[derive(Debug, Clone)]
pub enum FuncParamValue {
    /// Normalized value from an ident.
    Ident(String),
    /// Scalar value.
    Scalar(ScalarValue),
    /// A list of function parameter values.
    Array(Vec<FuncParamValue>),
}

impl fmt::Display for FuncParamValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Ident(s) => write!(f, "{s}"),
            Self::Scalar(s) => write!(f, "{s}"),
            Self::Array(vals) => write!(f, "{}", FuncParamValue::multiple_to_string(vals)),
        }
    }
}
impl TryFrom<FuncParamValue> for ProtoFuncParamValue {
    type Error = ExtensionError;

    fn try_from(value: FuncParamValue) -> std::result::Result<Self, Self::Error> {
        let variant = match value {
            FuncParamValue::Ident(id) => ProtoFuncParamValueEnum::Ident(id),
            FuncParamValue::Scalar(ref s) => ProtoFuncParamValueEnum::Scalar(s.try_into().unwrap()),
            FuncParamValue::Array(a) => {
                let values = a
                    .into_iter()
                    .map(|v| v.try_into())
                    .collect::<Result<Vec<_>, _>>()?;
                ProtoFuncParamValueEnum::Array(FuncParamValueArrayVariant { array: values })
            }
        };
        Ok(ProtoFuncParamValue {
            func_param_value_enum: Some(variant),
        })
    }
}

impl TryFrom<ProtoFuncParamValue> for FuncParamValue {
    type Error = ExtensionError;
    fn try_from(value: ProtoFuncParamValue) -> Result<Self, Self::Error> {
        let variant = value.func_param_value_enum.unwrap();
        Ok(match variant {
            ProtoFuncParamValueEnum::Ident(id) => Self::Ident(id),
            ProtoFuncParamValueEnum::Scalar(ref scalar) => {
                let scalar = scalar
                    .try_into()
                    .map_err(|_| ExtensionError::InvalidParamValue {
                        param: format!("{:?}", scalar),
                        expected: "scalar",
                    })?;
                Self::Scalar(scalar)
            }
            ProtoFuncParamValueEnum::Array(values) => {
                let values = values
                    .array
                    .into_iter()
                    .map(|v| v.try_into())
                    .collect::<Result<Vec<_>, _>>()?;
                Self::Array(values)
            }
        })
    }
}

impl FuncParamValue {
    /// Print multiple function parameter values.
    fn multiple_to_string<T: AsRef<[Self]>>(vals: T) -> String {
        use std::fmt::Write;

        let mut s = String::new();
        write!(&mut s, "(").unwrap();
        let mut sep = "";
        for val in vals.as_ref() {
            write!(&mut s, "{sep}{val}").unwrap();
            sep = ", ";
        }
        write!(&mut s, ")").unwrap();
        s
    }

    /// Wrapper over `FromFuncParamValue::from_param`.
    pub fn param_into<T>(self) -> Result<T>
    where
        T: FromFuncParamValue,
    {
        T::from_param(self)
    }

    /// Wrapper over `FromFuncParamValue::from_param`.
    pub fn param_ref_into<T>(&self) -> Result<T>
    where
        T: FromFuncParamValue,
    {
        T::from_param_ref(self)
    }
}

pub trait FromFuncParamValue: Sized {
    /// Get the value from parameter.
    fn from_param(value: FuncParamValue) -> Result<Self>;
    fn from_param_ref(value: &FuncParamValue) -> Result<Self> {
        Self::from_param(value.clone())
    }

    /// Check if the value is valid (able to convert).
    ///
    /// If `is_param_valid` returns true, `from_param` should be safely
    /// `unwrap`able (i.e., not panic).
    fn is_param_valid(value: &FuncParamValue) -> bool;
}

impl FromFuncParamValue for String {
    fn from_param(value: FuncParamValue) -> Result<Self> {
        match value {
            FuncParamValue::Scalar(ScalarValue::Utf8(Some(s))) => Ok(s),
            other => Err(ExtensionError::InvalidParamValue {
                param: other.to_string(),
                expected: "string",
            }),
        }
    }

    fn is_param_valid(value: &FuncParamValue) -> bool {
        matches!(value, FuncParamValue::Scalar(ScalarValue::Utf8(Some(_))))
    }
}

impl<T> FromFuncParamValue for Vec<T>
where
    T: FromFuncParamValue,
{
    fn from_param(value: FuncParamValue) -> Result<Self> {
        match value {
            FuncParamValue::Array(arr) => {
                let mut res = Vec::with_capacity(arr.len());
                for val in arr {
                    res.push(T::from_param(val)?);
                }
                Ok(res)
            }

            other => Err(ExtensionError::InvalidParamValue {
                param: other.to_string(),
                expected: "list",
            }),
        }
    }

    fn is_param_valid(value: &FuncParamValue) -> bool {
        if let FuncParamValue::Array(arr) = value {
            arr.iter().all(|v| T::is_param_valid(v))
        } else {
            false
        }
    }
}

pub struct IdentValue(String);

impl IdentValue {
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl Display for IdentValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl From<String> for IdentValue {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl From<IdentValue> for String {
    fn from(value: IdentValue) -> Self {
        value.0
    }
}

impl FromFuncParamValue for IdentValue {
    fn from_param(value: FuncParamValue) -> Result<Self> {
        match value {
            FuncParamValue::Ident(s) => Ok(Self(s)),
            other => Err(ExtensionError::InvalidParamValue {
                param: other.to_string(),
                expected: "ident",
            }),
        }
    }

    fn is_param_valid(value: &FuncParamValue) -> bool {
        matches!(value, FuncParamValue::Ident(_))
    }
}

impl FromFuncParamValue for i64 {
    fn from_param(value: FuncParamValue) -> Result<Self> {
        match value {
            FuncParamValue::Scalar(s) => match s {
                ScalarValue::Int8(Some(v)) => Ok(v as i64),
                ScalarValue::Int16(Some(v)) => Ok(v as i64),
                ScalarValue::Int32(Some(v)) => Ok(v as i64),
                ScalarValue::Int64(Some(v)) => Ok(v),
                ScalarValue::UInt8(Some(v)) => Ok(v as i64),
                ScalarValue::UInt16(Some(v)) => Ok(v as i64),
                ScalarValue::UInt32(Some(v)) => Ok(v as i64),
                ScalarValue::UInt64(Some(v)) => Ok(v as i64), // TODO: Handle overflow?
                other => Err(ExtensionError::InvalidParamValue {
                    param: other.to_string(),
                    expected: "integer",
                }),
            },

            other => Err(ExtensionError::InvalidParamValue {
                param: other.to_string(),
                expected: "integer",
            }),
        }
    }

    fn is_param_valid(value: &FuncParamValue) -> bool {
        matches!(
            value,
            FuncParamValue::Scalar(ScalarValue::Int8(Some(_)))
                | FuncParamValue::Scalar(ScalarValue::Int16(Some(_)))
                | FuncParamValue::Scalar(ScalarValue::Int32(Some(_)))
                | FuncParamValue::Scalar(ScalarValue::Int64(Some(_)))
                | FuncParamValue::Scalar(ScalarValue::UInt8(Some(_)))
                | FuncParamValue::Scalar(ScalarValue::UInt16(Some(_)))
                | FuncParamValue::Scalar(ScalarValue::UInt32(Some(_)))
                | FuncParamValue::Scalar(ScalarValue::UInt64(Some(_)))
        )
    }
}

impl FromFuncParamValue for f64 {
    fn from_param(value: FuncParamValue) -> Result<Self> {
        match value {
            FuncParamValue::Scalar(s) => match s {
                ScalarValue::Int8(Some(v)) => Ok(v as f64),
                ScalarValue::Int16(Some(v)) => Ok(v as f64),
                ScalarValue::Int32(Some(v)) => Ok(v as f64),
                ScalarValue::Int64(Some(v)) => Ok(v as f64),
                ScalarValue::UInt8(Some(v)) => Ok(v as f64),
                ScalarValue::UInt16(Some(v)) => Ok(v as f64),
                ScalarValue::UInt32(Some(v)) => Ok(v as f64),
                ScalarValue::UInt64(Some(v)) => Ok(v as f64),
                ScalarValue::Float32(Some(v)) => Ok(v as f64),
                ScalarValue::Float64(Some(v)) => Ok(v),
                other => Err(ExtensionError::InvalidParamValue {
                    param: other.to_string(),
                    expected: "double",
                }),
            },
            other => Err(ExtensionError::InvalidParamValue {
                param: other.to_string(),
                expected: "double",
            }),
        }
    }

    fn is_param_valid(value: &FuncParamValue) -> bool {
        matches!(
            value,
            FuncParamValue::Scalar(ScalarValue::Float32(Some(_)))
                | FuncParamValue::Scalar(ScalarValue::Float64(Some(_)))
        ) || i64::is_param_valid(value)
    }
}

impl FromFuncParamValue for Decimal128 {
    fn from_param(value: FuncParamValue) -> Result<Self> {
        match value {
            FuncParamValue::Scalar(s) => match s {
                ScalarValue::Int8(Some(v)) => Ok(Decimal128::try_from_int(v)?),
                ScalarValue::Int16(Some(v)) => Ok(Decimal128::try_from_int(v)?),
                ScalarValue::Int32(Some(v)) => Ok(Decimal128::try_from_int(v)?),
                ScalarValue::Int64(Some(v)) => Ok(Decimal128::try_from_int(v)?),
                ScalarValue::UInt8(Some(v)) => Ok(Decimal128::try_from_int(v)?),
                ScalarValue::UInt16(Some(v)) => Ok(Decimal128::try_from_int(v)?),
                ScalarValue::UInt32(Some(v)) => Ok(Decimal128::try_from_int(v)?),
                ScalarValue::UInt64(Some(v)) => Ok(Decimal128::try_from_int(v)?),
                ScalarValue::Float32(Some(v)) => Ok(Decimal128::try_from_float(v)?),
                ScalarValue::Float64(Some(v)) => Ok(Decimal128::try_from_float(v)?),
                ScalarValue::Decimal128(Some(v), _, s) => Ok(Decimal128::new(v, s)?),
                other => Err(ExtensionError::InvalidParamValue {
                    param: other.to_string(),
                    expected: "decimal",
                }),
            },
            other => Err(ExtensionError::InvalidParamValue {
                param: other.to_string(),
                expected: "decimal",
            }),
        }
    }

    fn is_param_valid(value: &FuncParamValue) -> bool {
        matches!(
            value,
            FuncParamValue::Scalar(ScalarValue::Decimal128(Some(_), _, _))
        ) || f64::is_param_valid(value)
            || i64::is_param_valid(value)
    }
}
