use std::str::FromStr;

use isbn2::Isbn;
use quick_epub::{IdentifierScheme, Metadata};

use crate::variant::{unravel_author, Identifier, MetadataFiller};

impl MetadataFiller for Metadata {
    fn take_title(&mut self, title: &mut Option<String>) {
        *title = std::mem::take(&mut self.title);
    }

    fn take_description(&mut self, description: &mut Option<String>) {
        *description = std::mem::take(&mut self.description);
    }

    fn take_language(&mut self, language: &mut Option<String>) {
        *language = std::mem::take(&mut self.language);
    }

    fn take_identifier(&mut self, identifier: &mut Option<Identifier>) {
        match std::mem::take(&mut self.identifier) {
            Some((id, value)) => match id {
                IdentifierScheme::ISBN => {
                    *identifier = Isbn::from_str(&value).ok().map(Identifier::ISBN)
                }
                IdentifierScheme::Unknown(id) => {
                    *identifier = Some(Identifier::Unknown(id, value));
                }
                x => *identifier = Some(Identifier::Unknown(x.to_string(), value)),
            },
            None => {}
        };
    }

    fn take_authors(&mut self, authors: &mut Option<Vec<String>>) {
        if let Some(author) = std::mem::take(&mut self.author) {
            *authors = Some(vec![unravel_author(&author)]);
        }
    }
}
