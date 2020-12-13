use std::fs::File;
use std::io::{BufReader, Error, Read};
use std::ops::Range;
use std::path::Path;

use zip::{result::ZipError, ZipArchive};

use regex::{bytes::Regex as ByteRegex, Regex};

use quick_xml::{events::Event, Reader};

// Not robust if string is escaped, but at the same time, who would do such a terrible thing in the
// root file?
fn get_root_file_byte_range(text: &[u8]) -> Option<Range<usize>> {
    lazy_static::lazy_static! {
        static ref RE: ByteRegex = ByteRegex::new(r#"(?:<rootfile )(?:[^>]*)(?:full-path=")([^"]*)(?:"[^>]*>)"#).unwrap();
    }
    if let Some(captures) = RE.captures(text) {
        if let Some(val) = captures.get(1) {
            Some(val.start()..val.end())
        } else {
            None
        }
    } else {
        None
    }
}

fn get_isbn(text: &str) -> Option<String> {
    lazy_static::lazy_static! {
        static ref RE: Regex = Regex::new(r#"(?:urn:isbn:)?([\d-]*)"#).unwrap();
    }
    if let Some(captures) = RE.captures(text) {
        if let Some(val) = captures.get(1) {
            Some(val.as_str().to_owned())
        } else {
            None
        }
    } else {
        None
    }
}

// Same as above, but again, what kind of terrible human being would put escaped metadata tags in
// their description? (Also, the first metadata tag being escaped is very, very unlikely).
fn get_metadata_byte_range(text: &[u8]) -> Option<Range<usize>> {
    lazy_static::lazy_static! {
        static ref RE: ByteRegex = ByteRegex::new(r#"(?s:<(?:opf:)?metadata[^>]*>)(?s)(.*)(?-s)(?:</(?:opf:)?metadata>)"#).unwrap();
    }
    if let Some(val) = RE.find(text) {
        Some(val.start()..val.end())
    } else {
        None
    }
}

pub enum EpubError {
    BadZip,
    IoError,
    NoContainer,
    NoRootFile,
    // NoContent,
    // BadMimetype,
    // NoMimetype,
    NoMetadata,
    BadXML,
}

impl From<std::io::Error> for EpubError {
    fn from(_: Error) -> Self {
        EpubError::IoError
    }
}

impl From<ZipError> for EpubError {
    fn from(_: ZipError) -> Self {
        EpubError::BadZip
    }
}

impl From<quick_xml::Error> for EpubError {
    fn from(_: quick_xml::Error) -> Self {
        EpubError::BadXML
    }
}

pub struct EpubMetadata {
    pub title: Option<String>,
    pub author: Option<String>,
    pub language: Option<String>,
    pub isbn: Option<String>,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum IdentifierScheme {
    Amazon,
    BarnesNoble,
    Calibre,
    EdelWeiss,
    FF,
    GoodReads,
    Google,
    ISBN,
    MOBIASIN,
    SonyBookID,
    URI,
    URL,
    URN,
    UUID,
    Unknown,
}

enum FieldSeen {
    Author,
    Title,
    Publisher,
    Identifier(IdentifierScheme),
    Language,
    None,
}

// TODO: Can have multi-part titles.

impl EpubMetadata {
    pub(crate) fn open<P: AsRef<Path>>(path: P) -> Result<Self, EpubError> {
        let mut buf = BufReader::new(File::open(path)?);
        let mut archive = ZipArchive::new(&mut buf)?;

        // if let Ok(mut mime) = archive.by_name("mimetype") {
        //     let expected = b"application/epub+zip".to_vec();
        //     let mut buf = vec![0; expected.len()];
        //     mime.read_to_end(&mut buf)?;
        //     println!("{}", String::from_utf8_lossy(&buf));
        //     if buf != expected {
        //         return Err(EpubError::BadMimetype);
        //     }
        // } else {
        //     return Err(EpubError::NoMimetype);
        // }

        let root_file = if let Ok(mut meta_inf) = archive.by_name("META-INF/container.xml") {
            let mut buf = Vec::new();
            meta_inf.read_to_end(&mut buf)?;
            // let s = String::from_utf8_lossy(&buf);
            match get_root_file_byte_range(&buf) {
                Some(range) => String::from_utf8_lossy(&buf[range]).into_owned(),
                None => return Err(EpubError::NoRootFile),
            }
        } else {
            return Err(EpubError::NoContainer);
        };

        let mut meta_buf = Vec::new();
        let range = if let Ok(mut root_file) = archive.by_name(&root_file) {
            root_file.read_to_end(&mut meta_buf)?;
            match get_metadata_byte_range(&meta_buf) {
                Some(metadata) => metadata,
                None => return Err(EpubError::NoMetadata),
            }
        } else {
            return Err(EpubError::NoContainer);
        };

        let mut new_obj = EpubMetadata {
            title: None,
            author: None,
            language: None,
            isbn: None,
        };

        let mut reader = Reader::from_reader(BufReader::new(&meta_buf[range]));
        reader.trim_text(true);

        let mut buf = Vec::new();

        let mut seen = FieldSeen::None;
        loop {
            match reader.read_event(&mut buf)? {
                Event::Start(ref e) => {
                    // println!(
                    //     "{} attributes values: {:?}",
                    //     String::from_utf8_lossy(e.name()),
                    //     e.attributes().map(|a| a.unwrap().value).collect::<Vec<_>>()
                    // );
                    seen = match e.name() {
                        b"dc:creator" => FieldSeen::Author,
                        b"dc:title" => FieldSeen::Title,
                        b"dc:identifier" => {
                            let mut id = None;
                            let mut scheme = None;
                            for a in e.attributes().filter_map(|a| a.ok()) {
                                match a.key {
                                    b"opf:scheme" => {
                                        scheme =
                                            Some(String::from_utf8_lossy(&a.value).into_owned())
                                    }
                                    b"id" => {
                                        id = Some(String::from_utf8_lossy(&a.value).into_owned())
                                    }
                                    _ => {}
                                }
                            }
                            FieldSeen::Identifier(match scheme {
                                None => match id {
                                    None => IdentifierScheme::Unknown,
                                    Some(val) => match val.to_ascii_lowercase().as_str() {
                                        "uuid_id" => IdentifierScheme::UUID,
                                        "isbn" => IdentifierScheme::ISBN,
                                        _ => IdentifierScheme::Unknown,
                                    },
                                },
                                Some(val) => match val.to_ascii_lowercase().as_str() {
                                    "amazon" => IdentifierScheme::Amazon,
                                    "barnesnoble" => IdentifierScheme::BarnesNoble,
                                    "calibre" => IdentifierScheme::Calibre,
                                    "edelweiss" => IdentifierScheme::EdelWeiss,
                                    "ff" => IdentifierScheme::FF,
                                    "goodreads" => IdentifierScheme::GoodReads,
                                    "google" => IdentifierScheme::Google,
                                    "isbn" => IdentifierScheme::ISBN,
                                    "mobi-asin" => IdentifierScheme::MOBIASIN,
                                    "sonybookid" => IdentifierScheme::SonyBookID,
                                    "uri" => IdentifierScheme::URI,
                                    "url" => IdentifierScheme::URL,
                                    "urn" => IdentifierScheme::URN,
                                    "uuid" => IdentifierScheme::UUID,
                                    _ => IdentifierScheme::Unknown,
                                },
                            })
                        }
                        b"dc:language" => FieldSeen::Language,
                        b"dc:publisher" => FieldSeen::Publisher,
                        _ => FieldSeen::None,
                    }
                }
                Event::Text(e) => {
                    let val = e.unescape_and_decode(&reader)?;
                    match seen {
                        FieldSeen::Author => {
                            new_obj.author = Some(val);
                        }
                        FieldSeen::Title => {
                            new_obj.title = Some(val);
                        }
                        FieldSeen::Publisher => {}
                        FieldSeen::Identifier(scheme) => match scheme {
                            IdentifierScheme::ISBN => new_obj.isbn = get_isbn(&val),
                            _ => {}
                        },
                        FieldSeen::Language => {
                            new_obj.language = Some(val);
                        }
                        FieldSeen::None => {}
                    }
                }
                Event::Eof => break,
                _ => {}
            }

            buf.clear();
        }

        Ok(new_obj)
    }
}

// TODO: https://www.oreilly.com/library/view/epub-3-best/9781449329129/ch01.html

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_get_isbn() {
        let isbn = "urn:isbn:0123456789012";
        assert_eq!(get_isbn(isbn), Some(String::from("0123456789012")));
        let isbn = "0123456789012";
        assert_eq!(get_isbn(isbn), Some(String::from("0123456789012")));
        let isbn = "978-0-345-53979-3";
        assert_eq!(get_isbn(isbn), Some(String::from("978-0-345-53979-3")));
    }
}
