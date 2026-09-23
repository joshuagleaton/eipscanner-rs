/// Declares a newtype over an integer wire code with named constants.
///
/// A newtype rather than an enum so that codes a device sends but this crate
/// doesn't know about still decode.
///
/// This input:
///
/// ```ignore
/// wire_code! {
///     /// Doc comment for the type.
///     Color(u8) {
///         RED = 1,
///         GREEN = 2,
///     }
/// }
/// ```
///
/// expands to roughly:
///
/// ```ignore
/// /// Doc comment for the type.
/// #[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
/// pub struct Color(pub u8);
///
/// impl Color {
///     pub const RED: Self = Self(1);
///     pub const GREEN: Self = Self(2);
///
///     pub fn name(self) -> Option<&'static str> {
///         match self {
///             Self::RED => Some("RED"),
///             Self::GREEN => Some("GREEN"),
///             _ => None,
///         }
///     }
/// }
///
/// impl Debug for Color { .. }    // "RED(0x1)", or "Color(0x7)" for unknown values
/// impl Display for Color { .. }  // same as Debug
/// impl From<u8> for Color { .. }
/// impl From<Color> for u8 { .. }
/// ```
///
/// Constants work in `match` patterns, and more can be added outside the macro
/// with a plain `impl Color { pub const BLUE: Self = Self(3); }`, though
/// `name()` won't know them.
macro_rules! wire_code {
    (
        $(#[$meta:meta])*
        $name:ident($repr:ty) {
            $( $(#[$cmeta:meta])* $konst:ident = $val:expr ),* $(,)?
        }
    ) => {
        $(#[$meta])*
        #[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
        pub struct $name(pub $repr);

        impl $name {
            $( $(#[$cmeta])* pub const $konst: Self = Self($val); )*

            /// Name of the code, if it is one this crate knows.
            pub fn name(self) -> Option<&'static str> {
                match self {
                    $( Self::$konst => Some(stringify!($konst)), )*
                    _ => None,
                }
            }
        }

        impl std::fmt::Debug for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                match self.name() {
                    Some(n) => write!(f, "{}({:#x})", n, self.0),
                    None => write!(f, "{}({:#x})", stringify!($name), self.0),
                }
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                std::fmt::Debug::fmt(self, f)
            }
        }

        impl From<$repr> for $name {
            fn from(v: $repr) -> Self {
                Self(v)
            }
        }

        impl From<$name> for $repr {
            fn from(v: $name) -> Self {
                v.0
            }
        }
    };
}
