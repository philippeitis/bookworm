use std::collections::HashMap;
use std::str::FromStr;

use isbn2::Isbn;
use mobi::MobiMetadata;

use crate::variant::{unravel_author, Identifier, MetadataFiller};

impl MetadataFiller for MobiMetadata {
    fn take_title(&mut self, title: &mut Option<String>) {
        *title = Some(self.title().unwrap_or(std::mem::take(&mut self.name)));
    }

    fn take_description(&mut self, description: &mut Option<String>) {
        *description = self.description();
    }

    fn take_language(&mut self, language: &mut Option<String>) {
        *language = self.language();
    }

    fn take_identifier(&mut self, identifier: &mut Option<Identifier>) {
        if let Some(isbn) = self.isbn() {
            if let Ok(isbn) = Isbn::from_str(&isbn) {
                *identifier = Some(Identifier::ISBN(isbn));
            }
        }
    }

    fn take_authors(&mut self, authors: &mut Option<Vec<String>>) {
        if let Some(author) = self.author() {
            *authors = Some(vec![unravel_author(&author)]);
        }
    }
}
