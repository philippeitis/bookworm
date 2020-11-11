use std::fmt;

use serde::{Deserialize, Serialize};

use crate::isbn::ISBN::ISBN10;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ISBNError {
    DigitTooLarge,
    UnrecognizedCompactU64Header,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub(crate) enum ISBN {
    ISBN10([u8; 10]),
    ISBN13([u8; 13]),
}

impl ISBN {
    pub(crate) fn to_compact_u64(&self) -> u64 {
        match self {
            ISBN::ISBN10(digits) => {
                let mut num = 0;
                for &digit in digits {
                    num <<= 5;
                    num |= digit as u64;
                }
                num |= 10 << 60;
                num
            }
            ISBN::ISBN13(digits) => {
                let mut num = digits[0] as u64;
                for i in 0..6 {
                    num <<= 7;
                    let high_digit = digits[1 + i * 2];
                    let low_digit = digits[2 + i * 2];
                    let dual_digit = high_digit * 10 + low_digit;
                    num |= dual_digit as u64;
                }
                num |= 13 << 60;
                num
            }
        }
    }

    pub(crate) fn from_compact_u64(mut num: u64) -> Result<ISBN, ISBNError> {
        if (num & (13 << 60)) == (13 << 60) {
            let mut digits = [0; 13];

            for i in 0..6 {
                let dual_digit = (num & 0b1111111) as u8;
                let low_digit = dual_digit % 10;
                let high_digit = dual_digit / 10;
                if dual_digit >= 100 {
                    return Err(ISBNError::DigitTooLarge);
                }

                num >>= 7;
                digits[13 - i * 2 - 1] = low_digit;
                digits[13 - i * 2 - 2] = high_digit;
            }

            let digit = (num & 0b11111) as u8;
            digits[0] = digit;
            Ok(ISBN::ISBN13(digits))
        } else if (num & (10 << 60)) == (10 << 60) {
            let mut digits = [0; 10];
            for i in 0..10 {
                let digit = (num & 0b11111) as u8;
                if digit >= 10 {
                    return Err(ISBNError::DigitTooLarge);
                }
                num >>= 5;
                digits[10 - i - 1] = digit;
            }
            Ok(ISBN10(digits))
        } else {
            Err(ISBNError::UnrecognizedCompactU64Header)
        }
    }

    pub(crate) fn isbn10_from_digits(digits: [u8; 10]) -> Result<ISBN, ISBNError> {
        if digits.iter().any(|x| *x > 10) {
            return Err(ISBNError::DigitTooLarge);
        }

        Ok(ISBN::ISBN10(digits))
    }

    pub(crate) fn isbn13_from_digits(digits: [u8; 13]) -> Result<ISBN, ISBNError> {
        if digits.iter().any(|x| *x > 10) {
            return Err(ISBNError::DigitTooLarge);
        }

        Ok(ISBN::ISBN13(digits))
    }

    pub(crate) fn check(self) -> bool {
        match self {
            ISBN::ISBN13(digits) => {
                let mut sum = 0u16;
                for i in 0..6 {
                    sum += (digits[i] + 3 * digits[i * 2 + 1]) as u16;
                }
                sum += digits[12] as u16;
                (sum % 10) == 0
            }
            ISBN::ISBN10(digits) => {
                let mut s = 0;
                let mut t = 0;

                for &digit in &digits {
                    t += digit as u16;
                    s += t;
                }
                (s % 11) == 0
            }
        }
    }

    pub(crate) fn to_digits(&self) -> Result<Vec<u8>, ISBNError> {
        match self {
            ISBN::ISBN13(digits) => Ok(digits.to_vec()),
            ISBN::ISBN10(digits) => Ok(digits.to_vec()),
        }
    }
}

impl fmt::Display for ISBN {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fn digit_to_str(digit: u8) -> char {
            char::from(digit + 48)
        }
        match self {
            ISBN::ISBN10(digits) => {
                let s: String = digits.into_iter().map(|&c| digit_to_str(c)).collect();
                write!(f, "{}", s)
            }
            ISBN::ISBN13(digits) => {
                let s: String = digits.into_iter().map(|&c| digit_to_str(c)).collect();
                write!(f, "{}", s)
            }
        }
    }
}

#[cfg(test)]
mod test {
    use super::ISBN;

    #[test]
    fn test_isbn13_check() {
        let digits = [9, 7, 8, 0, 3, 0, 6, 4, 0, 6, 1, 5, 7];
        let isbnx = ISBN::isbn13_from_digits(digits).unwrap();
        assert!(isbnx.check());
    }
}
