use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ISBNError {
    DigitTooLarge,
    UnrecognizedCompactU64Header,
    UnexpectedNumberOfDigits(usize),
    BadChecksum,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub enum ISBN {
    ISBN10([u8; 10]),
    ISBN13([u8; 13]),
}

impl ISBN {
    pub fn to_compact_u64(&self) -> u64 {
        match self {
            ISBN::ISBN10(digits) => {
                let mut num = 0;
                for &digit in digits {
                    num <<= 5;
                    num |= u64::from(digit);
                }
                num |= 10 << 60;
                num
            }
            ISBN::ISBN13(digits) => {
                let mut num = u64::from(digits[0]);
                for i in 0..6 {
                    num <<= 7;
                    let high_digit = digits[1 + i * 2];
                    let low_digit = digits[2 + i * 2];
                    let dual_digit = high_digit * 10 + low_digit;
                    num |= u64::from(dual_digit);
                }
                num |= 13 << 60;
                num
            }
        }
    }

    pub fn from_compact_u64(mut num: u64) -> Result<ISBN, ISBNError> {
        if (num & (13 << 60)) == (13 << 60) {
            let mut digits = [0; 13];

            for i in 0..6 {
                let dual_digit = (num & 0b111_1111) as u8;
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
            Ok(ISBN::ISBN10(digits))
        } else {
            Err(ISBNError::UnrecognizedCompactU64Header)
        }
    }

    pub fn isbn10_from_digits(digits: [u8; 10]) -> Result<ISBN, ISBNError> {
        if digits[..9].iter().any(|x| *x > 9) || digits[9] > 10 {
            return Err(ISBNError::DigitTooLarge);
        }

        Ok(ISBN::ISBN10(digits))
    }

    pub fn isbn13_from_digits(digits: [u8; 13]) -> Result<ISBN, ISBNError> {
        if digits.iter().any(|x| *x > 9) {
            return Err(ISBNError::DigitTooLarge);
        }

        Ok(ISBN::ISBN13(digits))
    }

    pub fn check(&self) -> bool {
        match self {
            ISBN::ISBN13(digits) => {
                let mut sum = 0;
                for i in 0..6 {
                    sum += u16::from(digits[i * 2] + 3 * digits[i * 2 + 1]);
                }
                sum += u16::from(digits[12]);
                (sum % 10) == 0
            }
            ISBN::ISBN10(digits) => {
                let mut s = 0;
                let mut t = 0;

                for &digit in digits {
                    t += u16::from(digit);
                    s += t;
                }
                (s % 11) == 0
            }
        }
    }

    pub fn to_digits(&self) -> Result<Vec<u8>, ISBNError> {
        match self {
            ISBN::ISBN13(digits) => Ok(digits.to_vec()),
            ISBN::ISBN10(digits) => Ok(digits.to_vec()),
        }
    }
}

impl FromStr for ISBN {
    type Err = ISBNError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let digits: Vec<_> = s
            .chars()
            .filter_map(|c| c.to_digit(10))
            .map(|d| d as u8)
            .collect();
        if digits.len() == 10 {
            let mut digit_arr = [0; 10];
            digit_arr.clone_from_slice(&digits);
            let isbn = ISBN::ISBN10(digit_arr);
            if isbn.check() {
                Ok(isbn)
            } else {
                Err(ISBNError::BadChecksum)
            }
        } else if digits.len() == 13 {
            let mut digit_arr = [0; 13];
            digit_arr.clone_from_slice(&digits);
            let isbn = ISBN::ISBN13(digit_arr);
            if isbn.check() {
                Ok(isbn)
            } else {
                Err(ISBNError::BadChecksum)
            }
        } else {
            Err(ISBNError::UnexpectedNumberOfDigits(digits.len()))
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
                let s: String = digits.iter().map(|&c| digit_to_str(c)).collect();
                write!(f, "{}", s)
            }
            ISBN::ISBN13(digits) => {
                let s: String = digits.iter().map(|&c| digit_to_str(c)).collect();
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
