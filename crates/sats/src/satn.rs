use crate::de::DeserializeSeed;
use crate::time_duration::TimeDuration;
use crate::timestamp::Timestamp;
use crate::{i256, u256, AlgebraicValue, WithTypespace};
use crate::{ser, ProductType, ProductTypeElement};
use core::fmt;
use core::fmt::Write as _;
use derive_more::{From, Into};

/// An extension trait for [`Serialize`](ser::Serialize) providing formatting methods.
pub trait Satn: ser::Serialize {
    /// Formats the value using the SATN data format into the formatter `f`.
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        Writer::with(f, |f| self.serialize(SatnFormatter { f }))?;
        Ok(())
    }

    /// Formats the value using the postgres SATN(PsqlFormatter { f }, /* PsqlType */) formatter `f`.
    fn fmt_psql(&self, f: &mut fmt::Formatter, ty: &PsqlType<'_>) -> fmt::Result {
        Writer::with(f, |f| {
            self.serialize(PsqlFormatter {
                fmt: SatnFormatter { f },
                ty,
            })
        })?;
        Ok(())
    }

    /// Formats the value using the SATN data format into the returned `String`.
    fn to_satn(&self) -> String {
        Wrapper::from_ref(self).to_string()
    }

    /// Pretty prints the value using the SATN data format into the returned `String`.
    fn to_satn_pretty(&self) -> String {
        format!("{:#}", Wrapper::from_ref(self))
    }
}

impl<T: ser::Serialize + ?Sized> Satn for T {}

/// A wrapper around a `T: Satn`
/// providing `Display` and `Debug` implementations
/// that uses the SATN formatting for `T`.
#[repr(transparent)]
pub struct Wrapper<T: ?Sized>(pub T);

impl<T: ?Sized> Wrapper<T> {
    /// Converts `&T` to `&Wrapper<T>`.
    pub fn from_ref(t: &T) -> &Self {
        // SAFETY: `repr(transparent)` turns the ABI of `T`
        // into the same as `Self` so we can also cast `&T` to `&Self`.
        unsafe { &*(t as *const T as *const Self) }
    }
}

impl<T: Satn + ?Sized> fmt::Display for Wrapper<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl<T: Satn + ?Sized> fmt::Debug for Wrapper<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// A wrapper around a `T: Satn`
/// providing `Display` and `Debug` implementations
/// that uses postgres SATN formatting for `T`.
pub struct PsqlWrapper<'a, T: ?Sized> {
    pub ty: PsqlType<'a>,
    pub value: T,
}

impl<T: ?Sized> PsqlWrapper<'_, T> {
    /// Converts `&T` to `&PsqlWrapper<T>`.
    pub fn from_ref(t: &T) -> &Self {
        // SAFETY: `repr(transparent)` turns the ABI of `T`
        // into the same as `Self` so we can also cast `&T` to `&Self`.
        unsafe { &*(t as *const T as *const Self) }
    }
}

impl<T: Satn + ?Sized> fmt::Display for PsqlWrapper<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.value.fmt_psql(f, &self.ty)
    }
}

impl<T: Satn + ?Sized> fmt::Debug for PsqlWrapper<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.value.fmt_psql(f, &self.ty)
    }
}

/// Wraps a writer for formatting lists separated by `SEP` into it.
struct EntryWrapper<'a, 'f, const SEP: char> {
    /// The writer we're formatting into.
    fmt: Writer<'a, 'f>,
    /// Whether there were any fields.
    /// Initially `false` and then `true` after calling [`.entry(..)`](EntryWrapper::entry).
    has_fields: bool,
}

impl<'a, 'f, const SEP: char> EntryWrapper<'a, 'f, SEP> {
    /// Constructs the entry wrapper using the writer `fmt`.
    fn new(fmt: Writer<'a, 'f>) -> Self {
        Self { fmt, has_fields: false }
    }

    /// Formats another entry in the larger structure.
    ///
    /// The formatting for the element / entry itself is provided by the function `entry`.
    fn entry(&mut self, entry: impl FnOnce(Writer) -> fmt::Result) -> fmt::Result {
        let res = (|| match &mut self.fmt {
            Writer::Pretty(f) => {
                if !self.has_fields {
                    f.write_char('\n')?;
                }
                f.state.indent += 1;
                entry(Writer::Pretty(f.as_mut()))?;
                f.write_char(SEP)?;
                f.write_char('\n')?;
                f.state.indent -= 1;
                Ok(())
            }
            Writer::Normal(f) => {
                if self.has_fields {
                    f.write_char(SEP)?;
                    f.write_char(' ')?;
                }
                entry(Writer::Normal(f))
            }
        })();
        self.has_fields = true;
        res
    }
}

/// An implementation of [`fmt::Write`] supporting indented and non-idented formatting.
enum Writer<'a, 'f> {
    /// Uses the standard library's formatter i.e. plain formatting.
    Normal(&'a mut fmt::Formatter<'f>),
    /// Uses indented formatting.
    Pretty(IndentedWriter<'a, 'f>),
}

impl<'f> Writer<'_, 'f> {
    /// Provided with a formatter `f`, runs `func` provided with a `Writer`.
    fn with<R>(f: &mut fmt::Formatter<'_>, func: impl FnOnce(Writer<'_, '_>) -> R) -> R {
        let mut state;
        // We use `alternate`, i.e., the `#` flag to let the user trigger pretty printing.
        let f = if f.alternate() {
            state = IndentState {
                indent: 0,
                on_newline: true,
            };
            Writer::Pretty(IndentedWriter { f, state: &mut state })
        } else {
            Writer::Normal(f)
        };
        func(f)
    }

    /// Returns a sub-writer without moving `self`.
    fn as_mut(&mut self) -> Writer<'_, 'f> {
        match self {
            Writer::Normal(f) => Writer::Normal(f),
            Writer::Pretty(f) => Writer::Pretty(f.as_mut()),
        }
    }
}

/// A formatter that adds decoration atop of the standard library's formatter.
struct IndentedWriter<'a, 'f> {
    f: &'a mut fmt::Formatter<'f>,
    state: &'a mut IndentState,
}

/// The indentation state.
struct IndentState {
    /// Number of tab indentations to make.
    indent: u32,
    /// Whether we were last on a newline.
    on_newline: bool,
}

impl<'f> IndentedWriter<'_, 'f> {
    /// Returns a sub-writer without moving `self`.
    fn as_mut(&mut self) -> IndentedWriter<'_, 'f> {
        IndentedWriter {
            f: self.f,
            state: self.state,
        }
    }
}

impl fmt::Write for IndentedWriter<'_, '_> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for s in s.split_inclusive('\n') {
            if self.state.on_newline {
                // Indent 4 characters times the indentation level.
                for _ in 0..self.state.indent {
                    self.f.write_str("    ")?;
                }
            }

            self.state.on_newline = s.ends_with('\n');
            self.f.write_str(s)?;
        }
        Ok(())
    }
}

impl fmt::Write for Writer<'_, '_> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        match self {
            Writer::Normal(f) => f.write_str(s),
            Writer::Pretty(f) => f.write_str(s),
        }
    }
}

/// Provides the SATN data format implementing [`Serializer`](ser::Serializer).
struct SatnFormatter<'a, 'f> {
    /// The sink / writer / output / formatter.
    f: Writer<'a, 'f>,
}

/// An error occurred during serialization to the SATS data format.
#[derive(From, Into)]
struct SatnError(fmt::Error);

impl ser::Error for SatnError {
    fn custom<T: fmt::Display>(_msg: T) -> Self {
        Self(fmt::Error)
    }
}

impl SatnFormatter<'_, '_> {
    /// Writes `args` formatted to `self`.
    #[inline(always)]
    fn write_fmt(&mut self, args: fmt::Arguments) -> Result<(), SatnError> {
        self.f.write_fmt(args)?;
        Ok(())
    }
}

impl<'a, 'f> ser::Serializer for SatnFormatter<'a, 'f> {
    type Ok = ();
    type Error = SatnError;
    type SerializeArray = ArrayFormatter<'a, 'f>;
    type SerializeSeqProduct = SeqFormatter<'a, 'f>;
    type SerializeNamedProduct = NamedFormatter<'a, 'f>;

    fn serialize_bool(mut self, v: bool) -> Result<Self::Ok, Self::Error> {
        write!(self, "{v}")
    }
    fn serialize_u8(mut self, v: u8) -> Result<Self::Ok, Self::Error> {
        write!(self, "{v}")
    }
    fn serialize_u16(mut self, v: u16) -> Result<Self::Ok, Self::Error> {
        write!(self, "{v}")
    }
    fn serialize_u32(mut self, v: u32) -> Result<Self::Ok, Self::Error> {
        write!(self, "{v}")
    }
    fn serialize_u64(mut self, v: u64) -> Result<Self::Ok, Self::Error> {
        write!(self, "{v}")
    }
    fn serialize_u128(mut self, v: u128) -> Result<Self::Ok, Self::Error> {
        write!(self, "{v}")
    }
    fn serialize_u256(mut self, v: u256) -> Result<Self::Ok, Self::Error> {
        write!(self, "{v}")
    }
    fn serialize_i8(mut self, v: i8) -> Result<Self::Ok, Self::Error> {
        write!(self, "{v}")
    }
    fn serialize_i16(mut self, v: i16) -> Result<Self::Ok, Self::Error> {
        write!(self, "{v}")
    }
    fn serialize_i32(mut self, v: i32) -> Result<Self::Ok, Self::Error> {
        write!(self, "{v}")
    }
    fn serialize_i64(mut self, v: i64) -> Result<Self::Ok, Self::Error> {
        write!(self, "{v}")
    }
    fn serialize_i128(mut self, v: i128) -> Result<Self::Ok, Self::Error> {
        write!(self, "{v}")
    }
    fn serialize_i256(mut self, v: i256) -> Result<Self::Ok, Self::Error> {
        write!(self, "{v}")
    }
    fn serialize_f32(mut self, v: f32) -> Result<Self::Ok, Self::Error> {
        write!(self, "{v}")
    }
    fn serialize_f64(mut self, v: f64) -> Result<Self::Ok, Self::Error> {
        write!(self, "{v}")
    }

    fn serialize_str(mut self, v: &str) -> Result<Self::Ok, Self::Error> {
        write!(self, "\"{v}\"")
    }

    fn serialize_bytes(mut self, v: &[u8]) -> Result<Self::Ok, Self::Error> {
        write!(self, "0x{}", hex::encode(v))
    }

    fn serialize_array(mut self, _len: usize) -> Result<Self::SerializeArray, Self::Error> {
        write!(self, "[")?; // Closed via `.end()`.
        Ok(ArrayFormatter {
            f: EntryWrapper::new(self.f),
        })
    }

    fn serialize_seq_product(self, len: usize) -> Result<Self::SerializeSeqProduct, Self::Error> {
        // Delegate to named products handling of element formatting.
        self.serialize_named_product(len).map(|inner| SeqFormatter { inner })
    }

    fn serialize_named_product(mut self, _len: usize) -> Result<Self::SerializeNamedProduct, Self::Error> {
        write!(self, "(")?; // Closed via `.end()`.
        Ok(NamedFormatter {
            f: EntryWrapper::new(self.f),
            idx: 0,
        })
    }

    fn serialize_variant<T: ser::Serialize + ?Sized>(
        mut self,
        _tag: u8,
        name: Option<&str>,
        value: &T,
    ) -> Result<Self::Ok, Self::Error> {
        write!(self, "(")?;
        EntryWrapper::<','>::new(self.f.as_mut()).entry(|mut f| {
            if let Some(name) = name {
                write!(f, "{name}")?;
            }
            write!(f, " = ")?;
            value.serialize(SatnFormatter { f })?;
            Ok(())
        })?;
        write!(self, ")")
    }
}

/// Defines the SATN formatting for arrays.
struct ArrayFormatter<'a, 'f> {
    /// The formatter for each element separating elements by a `,`.
    f: EntryWrapper<'a, 'f, ','>,
}

impl ser::SerializeArray for ArrayFormatter<'_, '_> {
    type Ok = ();
    type Error = SatnError;

    fn serialize_element<T: ser::Serialize + ?Sized>(&mut self, elem: &T) -> Result<(), Self::Error> {
        self.f.entry(|f| elem.serialize(SatnFormatter { f }).map_err(|e| e.0))?;
        Ok(())
    }

    fn end(mut self) -> Result<Self::Ok, Self::Error> {
        write!(self.f.fmt, "]")?;
        Ok(())
    }
}

/// Provides the data format for unnamed products for SATN.
struct SeqFormatter<'a, 'f> {
    /// Delegates to the named format.
    inner: NamedFormatter<'a, 'f>,
}

impl ser::SerializeSeqProduct for SeqFormatter<'_, '_> {
    type Ok = ();
    type Error = SatnError;

    fn serialize_element<T: ser::Serialize + ?Sized>(&mut self, elem: &T) -> Result<(), Self::Error> {
        ser::SerializeNamedProduct::serialize_element(&mut self.inner, None, elem)
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        ser::SerializeNamedProduct::end(self.inner)
    }
}

/// Provides the data format for named products for SATN.
struct NamedFormatter<'a, 'f> {
    /// The formatter for each element separating elements by a `,`.
    f: EntryWrapper<'a, 'f, ','>,
    /// The index of the element.
    idx: usize,
}

impl ser::SerializeNamedProduct for NamedFormatter<'_, '_> {
    type Ok = ();
    type Error = SatnError;

    fn serialize_element<T: ser::Serialize + ?Sized>(
        &mut self,
        name: Option<&str>,
        elem: &T,
    ) -> Result<(), Self::Error> {
        let res = self.f.entry(|mut f| {
            // Format the name or use the index if unnamed.
            if let Some(name) = name {
                write!(f, "{name}")?;
            } else {
                write!(f, "{}", self.idx)?;
            }
            write!(f, " = ")?;
            elem.serialize(SatnFormatter { f })?;
            Ok(())
        });
        self.idx += 1;
        res?;
        Ok(())
    }

    fn end(mut self) -> Result<Self::Ok, Self::Error> {
        write!(self.f.fmt, ")")?;
        Ok(())
    }
}

struct PsqlEntryWrapper<'a, 'f, const SEP: char> {
    entry: EntryWrapper<'a, 'f, SEP>,
    /// The index of the element.
    idx: usize,
    ty: &'a PsqlType<'a>,
}

/// Provides the data format for named products for `SQL`.
struct PsqlNamedFormatter<'a, 'f> {
    /// The formatter for each element separating elements by a `,`.
    f: PsqlEntryWrapper<'a, 'f, ','>,
    /// If is not [Self::is_special] to control if we start with `(`
    start: bool,
    /// Remember what format we are using
    use_fmt: PsqlPrintFmt,
}

impl<'a, 'f> PsqlNamedFormatter<'a, 'f> {
    pub fn new(ty: &'a PsqlType<'a>, f: Writer<'a, 'f>) -> Self {
        Self {
            start: true,
            f: PsqlEntryWrapper {
                entry: EntryWrapper::new(f),
                idx: 0,
                ty,
            },
            // Will set later
            use_fmt: PsqlPrintFmt::Satn,
        }
    }
}

impl ser::SerializeNamedProduct for PsqlNamedFormatter<'_, '_> {
    type Ok = ();
    type Error = SatnError;

    fn serialize_element<T: Satn + ser::Serialize + ?Sized>(
        &mut self,
        name: Option<&str>,
        elem: &T,
    ) -> Result<(), Self::Error> {
        // For binary data & special types, output in `hex` format and skip the tagging of each value
        // We need to check for both the  enclosing(`self.f.ty`) type and the inner element(`name`) type.
        self.use_fmt = self.f.ty.use_fmt(name);
        let res = self.f.entry.entry(|mut f| {
            let PsqlType { tuple, field, idx } = self.f.ty;
            if !self.use_fmt.is_special() {
                if self.start {
                    write!(f, "(")?;
                    self.start = false;
                }
                // Format the name or use the index if unnamed.
                if let Some(name) = name {
                    write!(f, "{name}")?;
                } else {
                    write!(f, "{idx}")?;
                }
                write!(f, " = ")?;
            }
            //Is a nested product type?
            let (tuple, field, idx) = if let Some(product) = field.algebraic_type.as_product() {
                (product, &product.elements[self.f.idx], self.f.idx)
            } else {
                (*tuple, *field, *idx)
            };

            elem.serialize(PsqlFormatter {
                fmt: SatnFormatter { f },
                ty: &PsqlType { tuple, field, idx },
            })?;

            Ok(())
        });

        // Advance to the next field.
        if !self.use_fmt.is_special() {
            self.f.idx += 1;
        }

        res?;

        Ok(())
    }

    fn end(mut self) -> Result<Self::Ok, Self::Error> {
        if !self.use_fmt.is_special() {
            write!(self.f.entry.fmt, ")")?;
        }
        Ok(())
    }
}

/// Provides the data format for unnamed products for `SQL`.
struct PsqlSeqFormatter<'a, 'f> {
    /// Delegates to the named format.
    inner: PsqlNamedFormatter<'a, 'f>,
}

impl ser::SerializeSeqProduct for PsqlSeqFormatter<'_, '_> {
    type Ok = ();
    type Error = SatnError;

    fn serialize_element<T: ser::Serialize + ?Sized>(&mut self, elem: &T) -> Result<(), Self::Error> {
        ser::SerializeNamedProduct::serialize_element(&mut self.inner, None, elem)
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        ser::SerializeNamedProduct::end(self.inner)
    }
}

/// How format of the `SQL` output?
#[derive(PartialEq)]
pub enum PsqlPrintFmt {
    /// Print as `hex` format
    Hex,
    /// Print as [`Timestamp`] format
    Timestamp,
    /// Print as [`TimeDuration`] format
    Duration,
    /// Print as `Satn` format
    Satn,
}

impl PsqlPrintFmt {
    fn is_special(&self) -> bool {
        self != &PsqlPrintFmt::Satn
    }
}

/// A wrapper that remember the `header` of the tuple/struct and the current field
#[derive(Debug, Clone)]
pub struct PsqlType<'a> {
    /// The header of the tuple/struct
    pub tuple: &'a ProductType,
    /// The current field
    pub field: &'a ProductTypeElement,
    /// The index of the field in the tuple/struct
    pub idx: usize,
}

impl PsqlType<'_> {
    /// Returns if the type is a special type
    ///
    /// Is required to check both the enclosing type and the inner element type
    fn use_fmt(&self, name: Option<&str>) -> PsqlPrintFmt {
        if self.tuple.is_identity()
            || self.tuple.is_connection_id()
            || self.field.algebraic_type.is_identity()
            || self.field.algebraic_type.is_connection_id()
            || name.map(ProductType::is_identity_tag).unwrap_or_default()
            || name.map(ProductType::is_connection_id_tag).unwrap_or_default()
        {
            return PsqlPrintFmt::Hex;
        };

        if self.tuple.is_timestamp()
            || self.field.algebraic_type.is_timestamp()
            || name.map(ProductType::is_timestamp_tag).unwrap_or_default()
        {
            return PsqlPrintFmt::Timestamp;
        };

        if self.tuple.is_time_duration()
            || self.field.algebraic_type.is_time_duration()
            || name.map(ProductType::is_time_duration_tag).unwrap_or_default()
        {
            return PsqlPrintFmt::Duration;
        };

        PsqlPrintFmt::Satn
    }
}

/// An implementation of [`Serializer`](ser::Serializer) for `SQL` output.
struct PsqlFormatter<'a, 'f> {
    fmt: SatnFormatter<'a, 'f>,
    ty: &'a PsqlType<'a>,
}

impl<'a, 'f> ser::Serializer for PsqlFormatter<'a, 'f> {
    type Ok = ();
    type Error = SatnError;
    type SerializeArray = ArrayFormatter<'a, 'f>;
    type SerializeSeqProduct = PsqlSeqFormatter<'a, 'f>;
    type SerializeNamedProduct = PsqlNamedFormatter<'a, 'f>;

    fn serialize_bool(self, v: bool) -> Result<Self::Ok, Self::Error> {
        self.fmt.serialize_bool(v)
    }
    fn serialize_u8(self, v: u8) -> Result<Self::Ok, Self::Error> {
        self.fmt.serialize_u8(v)
    }
    fn serialize_u16(self, v: u16) -> Result<Self::Ok, Self::Error> {
        self.fmt.serialize_u16(v)
    }
    fn serialize_u32(self, v: u32) -> Result<Self::Ok, Self::Error> {
        self.fmt.serialize_u32(v)
    }
    fn serialize_u64(self, v: u64) -> Result<Self::Ok, Self::Error> {
        self.fmt.serialize_u64(v)
    }
    fn serialize_u128(self, v: u128) -> Result<Self::Ok, Self::Error> {
        match self.ty.use_fmt(None) {
            PsqlPrintFmt::Hex => self.serialize_bytes(&v.to_be_bytes()),
            _ => self.fmt.serialize_u128(v),
        }
    }
    fn serialize_u256(self, v: u256) -> Result<Self::Ok, Self::Error> {
        match self.ty.use_fmt(None) {
            PsqlPrintFmt::Hex => self.serialize_bytes(&v.to_be_bytes()),
            _ => self.fmt.serialize_u256(v),
        }
    }
    fn serialize_i8(self, v: i8) -> Result<Self::Ok, Self::Error> {
        self.fmt.serialize_i8(v)
    }
    fn serialize_i16(self, v: i16) -> Result<Self::Ok, Self::Error> {
        self.fmt.serialize_i16(v)
    }
    fn serialize_i32(self, v: i32) -> Result<Self::Ok, Self::Error> {
        self.fmt.serialize_i32(v)
    }
    fn serialize_i64(mut self, v: i64) -> Result<Self::Ok, Self::Error> {
        match self.ty.use_fmt(None) {
            PsqlPrintFmt::Duration => {
                write!(self.fmt, "{}", TimeDuration::from_micros(v))?;
                Ok(())
            }
            PsqlPrintFmt::Timestamp => {
                write!(self.fmt, "{}", Timestamp::from_micros_since_unix_epoch(v))?;
                Ok(())
            }
            _ => self.fmt.serialize_i64(v),
        }
    }
    fn serialize_i128(self, v: i128) -> Result<Self::Ok, Self::Error> {
        self.fmt.serialize_i128(v)
    }
    fn serialize_i256(self, v: i256) -> Result<Self::Ok, Self::Error> {
        self.fmt.serialize_i256(v)
    }
    fn serialize_f32(self, v: f32) -> Result<Self::Ok, Self::Error> {
        self.fmt.serialize_f32(v)
    }
    fn serialize_f64(self, v: f64) -> Result<Self::Ok, Self::Error> {
        self.fmt.serialize_f64(v)
    }

    fn serialize_str(self, v: &str) -> Result<Self::Ok, Self::Error> {
        self.fmt.serialize_str(v)
    }

    fn serialize_bytes(self, v: &[u8]) -> Result<Self::Ok, Self::Error> {
        self.fmt.serialize_bytes(v)
    }

    fn serialize_array(self, len: usize) -> Result<Self::SerializeArray, Self::Error> {
        self.fmt.serialize_array(len)
    }

    fn serialize_seq_product(self, len: usize) -> Result<Self::SerializeSeqProduct, Self::Error> {
        Ok(PsqlSeqFormatter {
            inner: self.serialize_named_product(len)?,
        })
    }

    fn serialize_named_product(self, _len: usize) -> Result<Self::SerializeNamedProduct, Self::Error> {
        Ok(PsqlNamedFormatter::new(self.ty, self.fmt.f))
    }

    fn serialize_variant<T: ser::Serialize + ?Sized>(
        self,
        tag: u8,
        name: Option<&str>,
        value: &T,
    ) -> Result<Self::Ok, Self::Error> {
        self.fmt.serialize_variant(tag, name, value)
    }

    unsafe fn serialize_bsatn<Ty>(self, ty: &Ty, bsatn: &[u8]) -> Result<Self::Ok, Self::Error>
    where
        for<'b, 'de> WithTypespace<'b, Ty>: DeserializeSeed<'de, Output: Into<AlgebraicValue>>,
    {
        // SAFETY: Forward caller requirements of this method to that we are calling.
        unsafe { self.fmt.serialize_bsatn(ty, bsatn) }
    }

    unsafe fn serialize_bsatn_in_chunks<'c, Ty, I: Clone + Iterator<Item = &'c [u8]>>(
        self,
        ty: &Ty,
        total_bsatn_len: usize,
        bsatn: I,
    ) -> Result<Self::Ok, Self::Error>
    where
        for<'b, 'de> WithTypespace<'b, Ty>: DeserializeSeed<'de, Output: Into<AlgebraicValue>>,
    {
        // SAFETY: Forward caller requirements of this method to that we are calling.
        unsafe { self.fmt.serialize_bsatn_in_chunks(ty, total_bsatn_len, bsatn) }
    }

    unsafe fn serialize_str_in_chunks<'c, I: Clone + Iterator<Item = &'c [u8]>>(
        self,
        total_len: usize,
        string: I,
    ) -> Result<Self::Ok, Self::Error> {
        // SAFETY: Forward caller requirements of this method to that we are calling.
        unsafe { self.fmt.serialize_str_in_chunks(total_len, string) }
    }
}
