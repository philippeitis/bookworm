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
            // TODO: Replace with rsplit_once when stable (1.52).
            let mut words = s.rsplitn(2, char::is_whitespace);
            match (words.next(), words.next()) {
                (Some(id), Some(name)) => {
                    if id.starts_with('[') {
                        if let Ok(id) = f32::from_str(&id[1..id.len() - 1]) {
                            return Ok(Self {
                                name: name.to_owned(),
                                index: Some(id),
                            });
                        }
                    }
                }
                _ => {}
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
        self.name.eq(&other.name) && self.index.eq(&other.index)
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
                        (true, false) => Ordering::Greater,
                        (false, true) => Ordering::Less,
                        (true, true) => Ordering::Equal,
                    },
                },
            },
            ord => ord,
        }
    }
}
