use std::cmp::Ordering;
use std::fmt::{Display, Formatter};
use std::str::FromStr;

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[derive(Clone, Debug, Default)]
pub struct Series {
    pub name: String,
    pub index: Option<f32>,
}

impl Series {}

impl FromStr for Series {
    type Err = ();
    /// Parses a string of form `SeriesName [SeriesIndex]` into a book with series `SeriesName` and
    /// index `SeriesIndex`. If `SeriesIndex` can not be parsed as `f32`, or no brackets exist,
    /// the entire string is returned as `SeriesName`, with no associated `SeriesIndex`.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.ends_with(']') {
            if let Some((name, id)) = s.rsplit_once(char::is_whitespace) {
                if id.starts_with('[') {
                    if let Ok(id) = f32::from_str(&id[1..id.len() - 1]) {
                        return Ok(Self {
                            name: name.to_owned(),
                            index: Some(id),
                        });
                    }
                }
            }
        }

        Ok(Self {
            name: s.to_owned(),
            index: None,
        })
    }
}

impl Display for Series {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        if let Some(nth_in_series) = self.index {
            write!(f, "{} [{}]", self.name, nth_in_series)
        } else {
            write!(f, "{}", self.name)
        }
    }
}

impl std::cmp::PartialEq for Series {
    fn eq(&self, other: &Self) -> bool {
        self.partial_cmp(other) == Some(Ordering::Equal)
    }
}

impl std::cmp::Eq for Series {}

impl std::cmp::PartialOrd for Series {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl std::cmp::Ord for Series {
    fn cmp(&self, other: &Self) -> Ordering {
        match self.name.cmp(&other.name) {
            Ordering::Equal => match (self.index, other.index) {
                (None, None) => Ordering::Equal,
                (Some(_), None) => Ordering::Greater,
                (None, Some(_)) => Ordering::Less,
                (Some(si), Some(oi)) => match si.partial_cmp(&oi) {
                    Some(o) => o,
                    None => match (si.is_nan(), oi.is_nan()) {
                        (false, false) => unreachable!(
                            "Both can not be non-nan, otherwise partial_cmp would succeed"
                        ),
                        (true, false) => Ordering::Less,
                        (false, true) => Ordering::Greater,
                        (true, true) => Ordering::Equal,
                    },
                },
            },
            ord => ord,
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_series_from_str() {
        let a = Series::from_str("Hello World [1.0]").unwrap();
        assert_eq!(
            a,
            Series {
                name: "Hello World".to_string(),
                index: Some(1.)
            }
        );
    }

    #[test]
    fn test_series_from_single_bracket() {
        assert_eq!(
            Series::from_str("[").unwrap(),
            Series {
                name: "[".to_string(),
                index: None,
            }
        );

        assert_eq!(
            Series::from_str(" [").unwrap(),
            Series {
                name: " [".to_string(),
                index: None,
            }
        );
    }

    #[test]
    fn test_series_ordering() {
        let a = Series::from_str("Hello World").unwrap();
        let b = Series::from_str("Hello World [1]").unwrap();
        let c = Series::from_str("Hello World [2]").unwrap();
        let d = Series::from_str("World, Hello").unwrap();
        let e = Series::from_str("Hello World").unwrap();
        let f = Series {
            name: "Hello World".to_string(),
            index: Some(f32::NAN),
        };

        // Test no-index / no-index
        assert!(d > a);
        assert!(a < d);
        assert_eq!(a, e);

        // Test no-index / index
        assert!(a < b);
        assert!(b > a);
        assert!(d > b);
        assert_ne!(a, b);

        // Test index / index (non-nan)
        assert!(b < c);
        assert!(c > b);
        assert_eq!(b, b.clone());

        // Test index / index (nan)
        assert!(b > f);
        assert!(f < b);
        assert_eq!(f, f.clone());
    }
}
