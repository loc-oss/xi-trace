use std::fmt;
use std::string::ToString;

#[derive(Clone, Debug)]
pub enum CategoriesT {
    StaticArray(&'static [&'static str]),
    DynamicArray(Vec<String>),
}

trait StringArrayEq<Rhs: ?Sized = Self> {
    fn arr_eq(&self, other: &Rhs) -> bool;
}

impl StringArrayEq<[&'static str]> for Vec<String> {
    fn arr_eq(&self, other: &[&'static str]) -> bool {
        if self.len() != other.len() {
            return false;
        }

        for i in 0..self.len() {
            if self[i] != other[i] {
                return false;
            }
        }
        true
    }
}

impl StringArrayEq<Vec<String>> for &'static [&'static str] {
    fn arr_eq(&self, other: &Vec<String>) -> bool {
        if self.len() != other.len() {
            return false;
        }
        for i in 0..self.len() {
            if self[i] != other[i] {
                return false;
            }
        }
        true
    }
}

impl PartialEq for CategoriesT {
    fn eq(&self, other: &CategoriesT) -> bool {
        match *self {
            CategoriesT::StaticArray(ref self_arr) => match *other {
                CategoriesT::StaticArray(ref other_arr) => self_arr.eq(other_arr),
                CategoriesT::DynamicArray(ref other_arr) => self_arr.arr_eq(other_arr),
            },
            CategoriesT::DynamicArray(ref self_arr) => match *other {
                CategoriesT::StaticArray(other_arr) => self_arr.arr_eq(other_arr),
                CategoriesT::DynamicArray(ref other_arr) => self_arr.eq(other_arr),
            },
        }
    }
}

impl Eq for CategoriesT {}

impl serde::Serialize for CategoriesT {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.join(",").serialize(serializer)
    }
}

impl<'de> serde::Deserialize<'de> for CategoriesT {
    fn deserialize<D>(deserializer: D) -> Result<CategoriesT, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::Visitor;
        struct CategoriesTVisitor;

        impl<'de> Visitor<'de> for CategoriesTVisitor {
            type Value = CategoriesT;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("comma-separated strings")
            }

            fn visit_str<E>(self, v: &str) -> Result<CategoriesT, E>
            where
                E: serde::de::Error,
            {
                let categories = v.split(',').map(ToString::to_string).collect();
                Ok(CategoriesT::DynamicArray(categories))
            }
        }

        deserializer.deserialize_str(CategoriesTVisitor)
    }
}

impl CategoriesT {
    pub fn join(&self, sep: &str) -> String {
        match *self {
            CategoriesT::StaticArray(arr) => arr.join(sep),
            CategoriesT::DynamicArray(ref vec) => vec.join(sep),
        }
    }
}

macro_rules! categories_from_constant_array {
    ($num_args: expr) => {
        impl From<&'static [&'static str; $num_args]> for CategoriesT {
            fn from(c: &'static [&'static str; $num_args]) -> CategoriesT {
                CategoriesT::StaticArray(c)
            }
        }
    };
}

categories_from_constant_array!(0);
categories_from_constant_array!(1);
categories_from_constant_array!(2);
categories_from_constant_array!(3);
categories_from_constant_array!(4);
categories_from_constant_array!(5);
categories_from_constant_array!(6);
categories_from_constant_array!(7);
categories_from_constant_array!(8);
categories_from_constant_array!(9);
categories_from_constant_array!(10);

impl From<Vec<String>> for CategoriesT {
    fn from(c: Vec<String>) -> CategoriesT {
        CategoriesT::DynamicArray(c)
    }
}
