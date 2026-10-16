//! The value types behind the settings field table: Java's enums, the
//! per-layer settings, the dynamic [`FieldValue`] and the table entry
//! [`FieldDesc`].

use super::fields::SettingsPatch;

/// Java's `RouterScoringVersion`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RouterScoringVersion {
    V1Legacy,
    V2Continuous,
}

/// Java's `OptimizerScoringVersion`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum OptimizerScoringVersion {
    V1Legacy,
    V2LowerBound,
}

/// Java's `autoroute.BoardUpdateStrategy`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BoardUpdateStrategy {
    GlobalOptimal,
}

/// Java's `autoroute.ItemSelectionStrategy`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ItemSelectionStrategy {
    Sequential,
    Prioritized,
}

/// A Java enum: constants addressed by their Java names.
pub trait JavaEnum: Sized + Copy {
    /// The Java constant names, in declaration order.
    const NAMES: &'static [&'static str];
    fn java_name(self) -> &'static str;
    fn from_index(index: usize) -> Self;

    /// `Enum.name().equalsIgnoreCase(value)` over all constants.
    fn from_name_ignore_case(name: &str) -> Option<Self> {
        Self::NAMES
            .iter()
            .position(|n| crate::util::equals_ignore_case(n, name))
            .map(Self::from_index)
    }
}

macro_rules! java_enum {
    ($ty:ident { $($variant:ident = $name:literal),* }) => {
        impl JavaEnum for $ty {
            const NAMES: &'static [&'static str] = &[$($name),*];
            fn java_name(self) -> &'static str {
                match self { $($ty::$variant => $name),* }
            }
            fn from_index(index: usize) -> Self {
                [$($ty::$variant),*][index]
            }
        }
    };
}

java_enum!(RouterScoringVersion { V1Legacy = "V1_LEGACY", V2Continuous = "V2_CONTINUOUS" });
java_enum!(OptimizerScoringVersion { V1Legacy = "V1_LEGACY", V2LowerBound = "V2_LOWER_BOUND" });
java_enum!(BoardUpdateStrategy { GlobalOptimal = "GLOBAL_OPTIMAL" });
java_enum!(ItemSelectionStrategy { Sequential = "SEQUENTIAL", Prioritized = "PRIORITIZED" });

/// Java's `LayerSettings`: every member is nullable (a patch) and the
/// resolved settings keep the same type, because the board tuning of C03 reads
/// and writes exactly these `null`s.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LayerSettings {
    pub routable: Option<bool>,
    pub preferred_direction_horizontal: Option<bool>,
    pub bend_cost: Option<f64>,
    pub preferred_direction_trace_cost: Option<f64>,
    pub undesired_direction_trace_cost: Option<f64>,
}

impl LayerSettings {
    /// `new LayerSettings(routable, preferredDirectionHorizontal)`.
    pub fn new(routable: bool, preferred_direction_horizontal: bool) -> LayerSettings {
        LayerSettings {
            routable: Some(routable),
            preferred_direction_horizontal: Some(preferred_direction_horizontal),
            ..LayerSettings::default()
        }
    }

    /// `ReflectionUtil.copyFields(source, self)`: every non-null member of
    /// `source` overwrites this one. Returns the number of members copied.
    pub fn apply_from(&mut self, source: &LayerSettings) -> usize {
        let mut changed = 0;
        macro_rules! copy {
            ($f:ident) => {
                if source.$f.is_some() {
                    self.$f = source.$f;
                    changed += 1;
                }
            };
        }
        copy!(routable);
        copy!(preferred_direction_horizontal);
        copy!(bend_cost);
        copy!(preferred_direction_trace_cost);
        copy!(undesired_direction_trace_cost);
        changed
    }
}

/// Name, Java name and kind of a [`LayerSettings`] member, in Java's
/// declaration order.
pub struct LayerFieldDesc {
    pub name: &'static str,
    pub java_name: &'static str,
    pub kind: FieldKind,
    pub get: fn(&LayerSettings) -> Option<FieldValue>,
    pub set: fn(&mut LayerSettings, FieldValue),
}

/// The members of [`LayerSettings`].
pub static LAYER_FIELDS: &[LayerFieldDesc] = &[
    LayerFieldDesc {
        name: "routable",
        java_name: "routable",
        kind: FieldKind::Bool,
        get: |l| l.routable.map(FieldValue::Bool),
        set: |l, v| l.routable = bool::from_value(v),
    },
    LayerFieldDesc {
        name: "preferred_direction_horizontal",
        java_name: "preferredDirectionHorizontal",
        kind: FieldKind::Bool,
        get: |l| l.preferred_direction_horizontal.map(FieldValue::Bool),
        set: |l, v| l.preferred_direction_horizontal = bool::from_value(v),
    },
    LayerFieldDesc {
        name: "bend_cost",
        java_name: "bendCost",
        kind: FieldKind::Double,
        get: |l| l.bend_cost.map(FieldValue::Double),
        set: |l, v| l.bend_cost = f64::from_value(v),
    },
    LayerFieldDesc {
        name: "preferred_direction_trace_cost",
        java_name: "preferredDirectionTraceCost",
        kind: FieldKind::Double,
        get: |l| l.preferred_direction_trace_cost.map(FieldValue::Double),
        set: |l, v| l.preferred_direction_trace_cost = f64::from_value(v),
    },
    LayerFieldDesc {
        name: "undesired_direction_trace_cost",
        java_name: "undesiredDirectionTraceCost",
        kind: FieldKind::Double,
        get: |l| l.undesired_direction_trace_cost.map(FieldValue::Double),
        set: |l, v| l.undesired_direction_trace_cost = f64::from_value(v),
    },
];

/// The Java type of a settings field.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldKind {
    Bool,
    /// `Integer`.
    Int,
    /// `Long`.
    Long,
    /// `Float`.
    Float,
    /// `Double`.
    Double,
    Str,
    /// `String[]`.
    StrList,
    /// `double[]`.
    DoubleList,
    RouterScoringVersion,
    OptimizerScoringVersion,
    BoardUpdateStrategy,
    ItemSelectionStrategy,
}

/// A settings value of any field type.
#[derive(Clone, Debug, PartialEq)]
pub enum FieldValue {
    Bool(bool),
    Int(i32),
    Long(i64),
    Float(f32),
    Double(f64),
    Str(String),
    StrList(Vec<String>),
    DoubleList(Vec<f64>),
    /// An enum constant by its Java name.
    Enum(&'static str),
}

impl std::fmt::Display for FieldValue {
    /// Java's text for the value: `Float.toString`, `Double.toString`,
    /// comma-joined lists.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        use super::text::{java_double_to_string, java_float_to_string};
        match self {
            FieldValue::Bool(v) => write!(f, "{v}"),
            FieldValue::Int(v) => write!(f, "{v}"),
            FieldValue::Long(v) => write!(f, "{v}"),
            FieldValue::Float(v) => f.write_str(&java_float_to_string(*v)),
            FieldValue::Double(v) => f.write_str(&java_double_to_string(*v)),
            FieldValue::Str(v) => f.write_str(v),
            FieldValue::StrList(v) => f.write_str(&v.join(",")),
            FieldValue::DoubleList(v) => f.write_str(
                &v.iter()
                    .map(|d| java_double_to_string(*d))
                    .collect::<Vec<_>>()
                    .join(","),
            ),
            FieldValue::Enum(v) => f.write_str(v),
        }
    }
}

/// A Rust type that stands for a Java settings field type.
pub trait FieldType: Sized + Clone {
    const KIND: FieldKind;
    fn into_value(self) -> FieldValue;
    fn from_value(value: FieldValue) -> Option<Self>;
}

macro_rules! plain_field_type {
    ($ty:ty, $kind:ident, $variant:ident) => {
        impl FieldType for $ty {
            const KIND: FieldKind = FieldKind::$kind;
            fn into_value(self) -> FieldValue {
                FieldValue::$variant(self)
            }
            fn from_value(value: FieldValue) -> Option<Self> {
                match value {
                    FieldValue::$variant(v) => Some(v),
                    _ => None,
                }
            }
        }
    };
}

plain_field_type!(bool, Bool, Bool);
plain_field_type!(i32, Int, Int);
plain_field_type!(i64, Long, Long);
plain_field_type!(f32, Float, Float);
plain_field_type!(f64, Double, Double);
plain_field_type!(String, Str, Str);
plain_field_type!(Vec<String>, StrList, StrList);
plain_field_type!(Vec<f64>, DoubleList, DoubleList);

macro_rules! enum_field_type {
    ($ty:ident) => {
        impl FieldType for $ty {
            const KIND: FieldKind = FieldKind::$ty;
            fn into_value(self) -> FieldValue {
                FieldValue::Enum(self.java_name())
            }
            fn from_value(value: FieldValue) -> Option<Self> {
                match value {
                    FieldValue::Enum(name) => <$ty as JavaEnum>::from_name_ignore_case(name),
                    _ => None,
                }
            }
        }
    };
}

enum_field_type!(RouterScoringVersion);
enum_field_type!(OptimizerScoringVersion);
enum_field_type!(BoardUpdateStrategy);
enum_field_type!(ItemSelectionStrategy);

impl FieldValue {
    /// The constant named `name`, ignoring case, of the enum type `kind`.
    pub fn enum_constant(kind: FieldKind, name: &str) -> Option<FieldValue> {
        fn find<E: JavaEnum>(name: &str) -> Option<FieldValue> {
            E::from_name_ignore_case(name).map(|e| FieldValue::Enum(e.java_name()))
        }
        match kind {
            FieldKind::RouterScoringVersion => find::<RouterScoringVersion>(name),
            FieldKind::OptimizerScoringVersion => find::<OptimizerScoringVersion>(name),
            FieldKind::BoardUpdateStrategy => find::<BoardUpdateStrategy>(name),
            FieldKind::ItemSelectionStrategy => find::<ItemSelectionStrategy>(name),
            _ => None,
        }
    }
}

/// How a field is reachable as a flat `router.<name>` key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Flat {
    /// Not reachable as a flat key.
    No,
    /// Java's deprecated flat autorouter keys: the key works and warns.
    Deprecated,
    /// D8's flat cost keys: the key works without a warning.
    Quiet,
}

/// One entry of the settings table (see `fields.rs`).
pub struct FieldDesc {
    /// The canonical JSON path, for example `scoring.start_ripup_costs`.
    pub path: &'static str,
    /// The section (`scoring`), empty for a top-level field.
    pub section: &'static str,
    /// The JSON name inside the section (`start_ripup_costs`).
    pub name: &'static str,
    /// The Java field name (`startRipupCosts`).
    pub java_name: &'static str,
    /// The Gson alternate names.
    pub alternates: &'static [&'static str],
    /// Whether `router.<name>` is an alias of this field.
    pub flat: Flat,
    pub kind: FieldKind,
    /// False for Java's `transient` fields, which Gson neither reads nor
    /// writes.
    pub json: bool,
    /// Whether `resolve` can leave the field unset (`None` in the resolved
    /// view).
    pub optional: bool,
    pub doc: &'static str,
    /// The value in the patch, if set.
    pub get: fn(&SettingsPatch) -> Option<FieldValue>,
    /// Sets the patch value; a value of the wrong type is ignored.
    pub set: fn(&mut SettingsPatch, FieldValue),
    /// Unsets the patch value.
    pub clear: fn(&mut SettingsPatch),
    /// `DefaultSettings`' value for a machine with the given core count.
    pub default: fn(usize) -> Option<FieldValue>,
}

/// A section of the settings: a nested object such as `scoring`.
pub struct SectionDesc {
    pub name: &'static str,
    pub java_name: &'static str,
}
